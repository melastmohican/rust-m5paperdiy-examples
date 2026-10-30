//! # wifi_scan: Wi-Fi scan with esp-radio, blocking main (M5Stack PaperDIY)
//!
//! Proof of concept for the Arduino `PaperDIYUserDemo` Wi-Fi scan and the `FactoryTest`
//! Wi-Fi check. Stage 1: log only, no display. Starts the radio in station mode and scans
//! every 5 s, printing the SSID, signal strength and channel of up to 20 access points.
//!
//! The blocking twin of `wifi_scan` in `async/`. `esp-radio` has no
//! blocking scan in 0.18 and needs the `esp-rtos` scheduler, so this starts `esp-rtos` without
//! its Embassy executor and runs the async scan to completion with `embassy_futures::block_on`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example wifi_scan
//! ```
//!
//! ## Expected
//!
//! - `radio started`, then a line `scan N: M access points` every 5 s.
//! - One line per access point: `ssid, N dBm, channel N`. The FactoryTest passes when the
//!   strongest one is above -55 dBm.
#![no_std]
#![no_main]

extern crate alloc;

use defmt::{error, info};
use esp_hal::delay::Delay;
use esp_println as _;
use esp_radio::wifi::{ControllerConfig, scan::ScanConfig};

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    info!("wifi_scan: Rust, blocking");
    // The radio wants internal RAM, the sizes the `esp-radio` docs give. These regions go on the
    // `esp-alloc` that `esp-radio` and `esp-rtos` use, so this crate must depend on the same
    // version (0.10). With two copies there are two heaps, and the one that allocates is empty.
    rust_m5paperdiy_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_m5paperdiy_examples::esp_alloc::heap_allocator!(size: 36 * 1024);

    // The scheduler must run before the radio starts.
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    let delay = Delay::new();

    let Ok((mut controller, _interfaces)) =
        esp_radio::wifi::new(p.WIFI, ControllerConfig::default())
    else {
        error!("wifi init failed");
        rust_m5paperdiy_examples::halt();
    };
    info!("radio started");

    let config = ScanConfig::default().with_max(20);
    let mut round = 0u32;
    loop {
        match embassy_futures::block_on(controller.scan_async(&config)) {
            Ok(mut aps) => {
                aps.sort_by_key(|ap| core::cmp::Reverse(ap.signal_strength));
                info!("scan {}: {} access points", round, aps.len());
                for ap in &aps {
                    info!(
                        "  {}, {} dBm, channel {}",
                        ap.ssid.as_str(),
                        ap.signal_strength,
                        ap.channel
                    );
                }
            }
            Err(_) => error!("scan failed"),
        }
        round += 1;
        delay.delay_millis(5000);
    }
}
