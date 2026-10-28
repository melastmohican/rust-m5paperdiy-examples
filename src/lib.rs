//! Board support for the M5Stack PaperDIY used by the `epdpi` bring-up examples.
//!
//! Pin map and PMIC sequence come from M5GFX (`board_M5PaperDIY`) and FastEPD
//! (`BB_PANEL_M5PAPERS3`).
//!
//! | Signal | GPIO | Signal | GPIO |
//! |---|---|---|---|
//! | D0 | 6 | CKV | 18 |
//! | D1 | 14 | SPV | 17 |
//! | D2 | 7 | CL | 16 |
//! | D3 | 12 | LE | 15 |
//! | D4 | 9 | SPH | 13 |
//! | D5 | 11 | OE | 45 |
//! | D6 | 8 | PWR | 46 |
//! | D7 | 10 | I2C SDA / SCL | 41 / 42 |
#![no_std]

extern crate alloc;

pub use esp_alloc;
pub use esp_hal;

use embedded_hal::i2c::I2c;
use esp_hal::delay::Delay;
use esp_hal::gpio::Output;

use epdpi::prelude::*;

/// The bit-banged bus type every example uses.
pub type Bus = GenericGpioBus<Output<'static>, Delay>;

/// All fifteen display pins, configured as outputs and driven low.
pub struct PaperDiyPins {
    pub data: [Output<'static>; 8],
    pub control: ControlPins<Output<'static>>,
}

/// Parks the CPU after an unrecoverable setup error, so the log stays readable.
pub fn halt() -> ! {
    let delay = Delay::new();
    loop {
        delay.delay_millis(1000);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("{}", defmt::Display2Format(info));
    loop {}
}

/// Initializes the chip with PSRAM mapped into the heap and returns the peripherals.
///
/// The two 4 bits per pixel buffers are 259,200 bytes each, so PSRAM is required.
#[macro_export]
macro_rules! init {
    () => {{
        use $crate::esp_alloc::{HEAP, HeapRegion, MemoryCapability};
        use $crate::esp_hal::psram::{Psram, PsramConfig};
        let config = $crate::esp_hal::Config::default()
            .with_cpu_clock($crate::esp_hal::clock::CpuClock::max());
        let peripherals = $crate::esp_hal::init(config);
        // esp-alloc 0.9's `psram_allocator!` predates the esp-hal 1.1 PSRAM API, so the
        // region is registered by hand.
        let psram = Psram::new(
            unsafe { peripherals.PSRAM.clone_unchecked() },
            PsramConfig::default(),
        );
        let (start, size) = psram.raw_parts();
        unsafe {
            HEAP.add_region(HeapRegion::new(
                start,
                size,
                MemoryCapability::External.into(),
            ))
        };
        peripherals
    }};
}

/// Takes the display GPIOs out of `peripherals` and returns them as [`PaperDiyPins`].
#[macro_export]
macro_rules! display_pins {
    ($p:expr) => {{
        use $crate::esp_hal::gpio::AnyPin;
        use $crate::esp_hal::gpio::{Level, Output, OutputConfig};
        let out = |pin: AnyPin<'static>| Output::new(pin, Level::Low, OutputConfig::default());
        $crate::PaperDiyPins {
            data: [
                out($p.GPIO6.into()),
                out($p.GPIO14.into()),
                out($p.GPIO7.into()),
                out($p.GPIO12.into()),
                out($p.GPIO9.into()),
                out($p.GPIO11.into()),
                out($p.GPIO8.into()),
                out($p.GPIO10.into()),
            ],
            control: $crate::epdpi::ControlPins {
                ckv: out($p.GPIO18.into()),
                spv: out($p.GPIO17.into()),
                sph: out($p.GPIO13.into()),
                oe: out($p.GPIO45.into()),
                le: out($p.GPIO15.into()),
                cl: out($p.GPIO16.into()),
                pwr: out($p.GPIO46.into()),
            },
        }
    }};
}

pub use epdpi;

impl PaperDiyPins {
    /// Wraps the pins in the portable bit-banged bus. Correct but about 15 times slower per
    /// row than M5GFX, which saturates the mid grays. Fine for `Clear` and `FastMono`.
    pub fn into_bus(self) -> Bus {
        GenericGpioBus::new(self.data, self.control, Delay::new())
    }

    /// Wraps the pins in the register-level bus, with row timing close to M5GFX.
    pub fn into_fast_bus(self) -> fast_bus::FastBus {
        fast_bus::FastBus::new(self)
    }
}

/// ESP32-S3 bus that writes scanlines through the GPIO set/clear registers.
///
/// `GenericGpioBus` takes about 250 us per row because every data pin is a separate call.
/// The ED047TC1 waveform matrix assumes a row time near M5GFX's 16 us (FastEPD runs the same
/// glass at 40 MHz, 6 us). A row that long drives every pass too hard, and the mid grays
/// saturate. Data pins D0..D7 and CL are all below GPIO32, so one store sets or clears them.
pub mod fast_bus {
    use core::convert::Infallible;

    use esp_hal::delay::Delay;
    use esp_hal::gpio::Output;

    use super::PaperDiyPins;
    use epdpi::prelude::*;

    /// ESP32-S3 GPIO_OUT_W1TS_REG and GPIO_OUT_W1TC_REG (TRM, GPIO base 0x6000_4000).
    const OUT_W1TS: *mut u32 = 0x6000_4008 as *mut u32;
    const OUT_W1TC: *mut u32 = 0x6000_400C as *mut u32;

    /// GPIO number of D0..D7. Must match [`display_pins!`](crate::display_pins).
    const DATA_GPIO: [u32; 8] = [6, 14, 7, 12, 9, 11, 8, 10];
    const CL_GPIO: u32 = 16;

    /// See the module docs.
    pub struct FastBus {
        // Held so the pins stay configured as outputs.
        _data: [Output<'static>; 8],
        ckv: Output<'static>,
        spv: Output<'static>,
        sph: Output<'static>,
        oe: Output<'static>,
        le: Output<'static>,
        _cl: Output<'static>,
        pwr: Output<'static>,
        delay: Delay,
        /// GPIO bit pattern for each byte value on D0..D7.
        table: [u32; 256],
        all_data: u32,
    }

    impl FastBus {
        pub(crate) fn new(pins: PaperDiyPins) -> Self {
            let mut table = [0u32; 256];
            for (value, slot) in table.iter_mut().enumerate() {
                for (bit, gpio) in DATA_GPIO.iter().enumerate() {
                    if value & (1 << bit) != 0 {
                        *slot |= 1 << gpio;
                    }
                }
            }
            let all_data = DATA_GPIO.iter().fold(0, |m, g| m | (1 << g));
            let c = pins.control;
            Self {
                _data: pins.data,
                ckv: c.ckv,
                spv: c.spv,
                sph: c.sph,
                oe: c.oe,
                le: c.le,
                _cl: c.cl,
                pwr: c.pwr,
                delay: Delay::new(),
                table,
                all_data,
            }
        }
    }

    impl EpdParallelBus for FastBus {
        type Error = Infallible;

        fn set_power(&mut self, on: bool) -> Result<(), Self::Error> {
            if on {
                self.oe.set_high();
                self.delay.delay_micros(100);
                self.pwr.set_high();
                self.delay.delay_micros(100);
                self.spv.set_high();
                self.sph.set_high();
                self.delay.delay_millis(1);
            } else {
                self.delay.delay_millis(1);
                self.pwr.set_low();
                self.delay.delay_micros(10);
                self.oe.set_low();
                self.delay.delay_micros(100);
                self.spv.set_low();
            }
            Ok(())
        }

        fn start_frame(&mut self) -> Result<(), Self::Error> {
            self.spv.set_low();
            self.delay.delay_micros(1);
            self.ckv.set_low();
            self.delay.delay_micros(3);
            self.ckv.set_high();
            self.delay.delay_micros(1);
            self.spv.set_high();
            for _ in 0..3 {
                self.delay.delay_micros(3);
                self.ckv.set_low();
                self.delay.delay_micros(3);
                self.ckv.set_high();
            }
            Ok(())
        }

        fn write_scanline(&mut self, data: &[u8]) -> Result<(), Self::Error> {
            self.le.set_low();
            self.ckv.set_high();
            self.sph.set_low();
            let cl = 1u32 << CL_GPIO;
            for &byte in data {
                let ones = self.table[usize::from(byte)];
                // SAFETY: fixed ESP32-S3 GPIO write-one-to-set/clear registers. A write only
                // affects the bits set in the value, and the pins touched here (D0..D7, CL)
                // are owned by this struct.
                unsafe {
                    OUT_W1TC.write_volatile(self.all_data & !ones);
                    OUT_W1TS.write_volatile(ones);
                    OUT_W1TS.write_volatile(cl);
                    OUT_W1TC.write_volatile(cl);
                }
            }
            self.sph.set_high();
            Ok(())
        }

        fn next_row(&mut self) -> Result<(), Self::Error> {
            self.ckv.set_low();
            self.le.set_high();
            Ok(())
        }

        fn end_frame(&mut self) -> Result<(), Self::Error> {
            self.le.set_low();
            self.ckv.set_high();
            Ok(())
        }
    }
}

/// M5PM1 power management IC, the only way to switch the panel rail on a PaperDIY.
pub mod pmic {
    use super::*;

    /// I2C address. M5GFX uses 0x6E. The M5IOE1 expander on other boards sits at 0x6F.
    pub const ADDR: u8 = 0x6E;
    /// Value of registers 0x00..0x01 (little endian) on a genuine M5PM1.
    pub const DEVICE_ID: u16 = 0x2050;

    const REG_ID: u8 = 0x00;
    const REG_GPIO_MODE: u8 = 0x10;
    const REG_GPIO_OUT: u8 = 0x11;
    const REG_GPIO_DRV: u8 = 0x13;
    const REG_GPIO_FUNC: u8 = 0x16;
    /// GPIO2 gates EPD_PWR.
    const EPD_PWR_BIT: u8 = 1 << 2;

    /// Reads the device ID register pair.
    pub fn read_id<I: I2c>(i2c: &mut I) -> Result<u16, I::Error> {
        let mut id = [0u8; 2];
        i2c.write_read(ADDR, &[REG_ID], &mut id)?;
        Ok(u16::from_le_bytes(id))
    }

    fn update<I: I2c>(i2c: &mut I, reg: u8, clear: u8, set: u8) -> Result<(), I::Error> {
        let mut cur = [0u8; 1];
        i2c.write_read(ADDR, &[reg], &mut cur)?;
        i2c.write(ADDR, &[reg, (cur[0] & !clear) | set])
    }

    /// Makes GPIO2 a push-pull output and drives it high, as M5GFX does at board detect.
    pub fn epd_power_on<I: I2c>(i2c: &mut I, delay: &mut Delay) -> Result<(), I::Error> {
        use embedded_hal::delay::DelayNs;
        // Clear idle-sleep settings the PMIC may keep across a battery shutdown.
        i2c.write(ADDR, &[0x09, 0x00])?;
        i2c.write(ADDR, &[0x0A, 0x00])?;
        update(i2c, REG_GPIO_FUNC, 0b11 << 4, 0)?; // GPIO2 pin function: plain GPIO
        update(i2c, REG_GPIO_MODE, 0, EPD_PWR_BIT)?; // output
        update(i2c, REG_GPIO_DRV, EPD_PWR_BIT, 0)?; // push-pull
        update(i2c, REG_GPIO_OUT, 0, EPD_PWR_BIT)?; // high
        delay.delay_ms(10);
        Ok(())
    }

    /// Drives GPIO2 low.
    pub fn epd_power_off<I: I2c>(i2c: &mut I) -> Result<(), I::Error> {
        update(i2c, REG_GPIO_OUT, EPD_PWR_BIT, 0)
    }
}

/// Opens the PMIC I2C bus (SDA GPIO41, SCL GPIO42, 100 kHz).
#[macro_export]
macro_rules! pmic_i2c {
    ($p:expr) => {{
        use $crate::esp_hal::i2c::master::{Config, I2c};
        use $crate::esp_hal::time::Rate;
        match I2c::new(
            $p.I2C0,
            Config::default().with_frequency(Rate::from_khz(100)),
        ) {
            Ok(i2c) => i2c.with_sda($p.GPIO41).with_scl($p.GPIO42),
            Err(_) => panic!("I2C0 config rejected"),
        }
    }};
}
