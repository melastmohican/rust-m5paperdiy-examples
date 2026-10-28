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

/// The portable bit-banged bus type.
pub type Bus = GenericGpioBus<Output<'static>, Delay>;

/// The register-level bus type the examples with grays use.
pub type FastBus = epdpi::fast_bus::Esp32s3Bus<Output<'static>, Delay>;

/// GPIO numbers of D0..D7 and CL. Must match [`display_pins!`].
const DATA_GPIO: [u8; 8] = [6, 14, 7, 12, 9, 11, 8, 10];
const CL_GPIO: u8 = 16;

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

    /// Wraps the pins in `epdpi`'s register-level bus, with row timing close to M5GFX.
    pub fn into_fast_bus(self) -> FastBus {
        match FastBus::new(self.data, DATA_GPIO, self.control, CL_GPIO, Delay::new()) {
            Ok(bus) => bus,
            Err(_) => panic!("PaperDIY pin map rejected by the fast bus"),
        }
    }
}

/// ESP32-S3 `LCD_CAM` i8080 bus with DMA, the way M5GFX drives the panel.
///
/// The data pins D0..D7, the source clock CL (the i8080 write strobe) and SPH (the chip
/// select, low while a scanline is clocked) belong to the LCD peripheral. CKV, LE, SPV, OE
/// and PWR stay plain GPIO, with CKV held high for exactly the length of the DMA transfer,
/// as M5GFX's `Bus_EPD` does. The row data phase is then fixed by the bus clock.
pub mod lcd_bus {
    use esp_hal::Blocking;
    use esp_hal::delay::Delay;
    use esp_hal::dma::{DmaError, DmaTxBuf};
    use esp_hal::gpio::Output;
    use esp_hal::lcd_cam::lcd::i8080::{Command, I8080};

    use epdpi::prelude::*;

    /// Why a bus call failed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LcdBusError {
        /// The DMA transfer failed.
        Dma(DmaError),
        /// A previous failure left the bus without its peripheral.
        Lost,
        /// The scanline does not fit the DMA buffer.
        TooLong,
    }

    /// See the module docs.
    pub struct LcdBus {
        i8080: Option<I8080<'static, Blocking>>,
        buf: Option<DmaTxBuf>,
        ckv: Output<'static>,
        spv: Output<'static>,
        oe: Output<'static>,
        le: Output<'static>,
        pwr: Output<'static>,
        delay: Delay,
    }

    impl LcdBus {
        #[doc(hidden)]
        pub fn new(
            i8080: I8080<'static, Blocking>,
            buf: DmaTxBuf,
            control: [Output<'static>; 5],
        ) -> Self {
            let [ckv, spv, oe, le, pwr] = control;
            Self {
                i8080: Some(i8080),
                buf: Some(buf),
                ckv,
                spv,
                oe,
                le,
                pwr,
                delay: Delay::new(),
            }
        }
    }

    impl EpdParallelBus for LcdBus {
        type Error = LcdBusError;

        fn set_power(&mut self, on: bool) -> Result<(), Self::Error> {
            if on {
                self.oe.set_high();
                self.delay.delay_micros(100);
                self.pwr.set_high();
                self.delay.delay_micros(100);
                self.spv.set_high();
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
            let (Some(i8080), Some(mut buf)) = (self.i8080.take(), self.buf.take()) else {
                return Err(LcdBusError::Lost);
            };
            if data.len() > buf.capacity() {
                self.i8080 = Some(i8080);
                self.buf = Some(buf);
                return Err(LcdBusError::TooLong);
            }
            buf.fill(data);
            self.le.set_low();
            self.ckv.set_high();
            match i8080.send(Command::<u8>::None, 0, buf) {
                Ok(transfer) => {
                    let (result, i8080, buf) = transfer.wait();
                    self.i8080 = Some(i8080);
                    self.buf = Some(buf);
                    result.map_err(LcdBusError::Dma)
                }
                Err((err, i8080, buf)) => {
                    self.i8080 = Some(i8080);
                    self.buf = Some(buf);
                    Err(LcdBusError::Dma(err))
                }
            }
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

/// Builds an [`lcd_bus::LcdBus`] on the PaperDIY pins, with the bus clock in MHz.
///
/// The data lines D0..D7 are GPIO 6, 14, 7, 12, 9, 11, 8, 10, CL is GPIO16 and SPH is
/// GPIO13, all driven by the LCD peripheral. CKV 18, SPV 17, OE 45, LE 15 and PWR 46 are
/// outputs. Uses `DMA_CH0` and `LCD_CAM`.
#[macro_export]
macro_rules! lcd_bus {
    ($p:expr, $mhz:expr) => {{
        use $crate::esp_hal::dma_tx_buffer;
        use $crate::esp_hal::gpio::{Level, Output, OutputConfig};
        use $crate::esp_hal::lcd_cam::LcdCam;
        use $crate::esp_hal::lcd_cam::lcd::i8080::{Config, I8080};
        use $crate::esp_hal::time::Rate;
        let out = |pin: $crate::esp_hal::gpio::AnyPin<'static>| {
            Output::new(pin, Level::Low, OutputConfig::default())
        };
        let lcd_cam = LcdCam::new($p.LCD_CAM);
        let config = Config::default().with_frequency(Rate::from_mhz($mhz));
        let Ok(i8080) = I8080::new(lcd_cam.lcd, $p.DMA_CH0, config) else {
            panic!("LCD_CAM config rejected");
        };
        let i8080 = i8080
            .with_wrx($p.GPIO16)
            .with_cs($p.GPIO13)
            .with_data0($p.GPIO6)
            .with_data1($p.GPIO14)
            .with_data2($p.GPIO7)
            .with_data3($p.GPIO12)
            .with_data4($p.GPIO9)
            .with_data5($p.GPIO11)
            .with_data6($p.GPIO8)
            .with_data7($p.GPIO10);
        let Ok(buf) = dma_tx_buffer!(256) else {
            panic!("DMA buffer allocation failed");
        };
        $crate::lcd_bus::LcdBus::new(
            i8080,
            buf,
            [
                out($p.GPIO18.into()),
                out($p.GPIO17.into()),
                out($p.GPIO45.into()),
                out($p.GPIO15.into()),
                out($p.GPIO46.into()),
            ],
        )
    }};
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
