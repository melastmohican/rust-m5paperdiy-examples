# rust-m5paperdiy-examples

Rust examples for the **M5Stack PaperDIY** (ESP32-S3, 4.7" ED047TC1 960 x 540 e-paper,
16 grays, two buttons, microSD, M5PM1 power chip), built on the
[`epdpi`](../epdpi) driver. Each file in `examples/` is a complete `no_std` image. Every port
of an Arduino sketch also exists as an async (Embassy) twin of the same name in the sibling
project [`rust-m5paperdiy-embassy-examples`](https://github.com/melastmohican/rust-m5paperdiy-embassy-examples).

## Hello M5PaperDIY!

`src/main.rs` is the smallest real program: it shows "Hello M5PaperDIY!" in the middle of the
screen and stops.

```bash
cargo run --release
```

It is the quickest check that a board, cable and toolchain work. If nothing appears, work through the
bring-up diagnostics in [bringup.md](bringup.md).

## Kinds of example

| Name | Purpose | Status |
| :--- | :--- | :--- |
| no prefix | Ports of the Arduino sketches in [m5paperdiy-arduino-examples](https://github.com/melastmohican/m5paperdiy-arduino-examples) | `display_demo`, `buttons`, `text_log_scroll`, `factory_test`, `user_demo`, all working, see [Ports](#ports-of-the-arduino-sketches) |
| `poc_*`, `wifi_scan` | Proofs that a part works from Rust, with a log and no display | `poc_battery`, `poc_sd`, `wifi_scan`, all working |

The diagnostics that proved the `epdpi` driver layer by layer, and the host tests, are described in
[bringup.md](bringup.md). Their code is on the `bringup` branch.

## Run

```bash
cargo run --release --example user_demo             # flash and monitor via espflash
cargo build --release --bins --examples             # build everything
cargo clippy --bins --examples --workspace -- -D warnings  # CI lint
```

Dependencies: `[dependencies]` holds only what the library and `src/main.rs` need. Anything
an example alone needs (SD card, font crates, image decoders) goes in `[dev-dependencies]`.

Requirements: the `esp` toolchain from `espup` (`rustup toolchain list` should show `esp`)
and `espflash`. Always build with `--release`: the PSRAM heap and the bus are far too slow in
a dev build. `epdpi` and `epdpi-esp` are path dependencies on the sibling checkout `../epdpi`, a workspace with both crates.

## Board reference

| Signal | GPIO | Signal | GPIO |
| :--- | :--- | :--- | :--- |
| D0..D7 | 6, 14, 7, 12, 9, 11, 8, 10 | CKV | 18 |
| SPV | 17 | CL | 16 |
| LE | 15 | SPH | 13 |
| OE | 45 | PWR | 46 |
| I2C SDA / SCL | 41 / 42 | M5PM1 address | 0x6E |
| BtnA / BtnB | 4 / 3 (active low) | microSD CLK / MOSI / MISO / CS | 39 / 38 / 40 / 47 |
| microSD detect | 48 (low when a card is in) | | |

The panel rail is gated twice: PMIC GPIO2 (EPD_PWR) over I2C and ESP32 GPIO46. Every display
example switches the PMIC on first through `pmic::epd_power_on`. Pin map and PMIC sequence
come from M5GFX (`board_M5PaperDIY`). The button and microSD pins come from M5Unified and
the Arduino sketches, and `buttons`, `factory_test` and `user_demo` use them.

## Notes for writing examples

These cost time to find. They are recorded here so the next person does not repeat them.

- **PSRAM is required.** The two 4 bits per pixel buffers are 259,200 bytes each. The chip
  has 8 MB of octal PSRAM. The `init!` macro registers the region with `esp-alloc` by hand.
- **Keep one `esp-alloc`.** `esp-radio` and `esp-rtos` use `esp-alloc` 0.10, so this crate does
  too. With two versions in the tree there are two heaps: `heap_allocator!` fills one, the global
  allocator is the other and is empty, and the first Wi-Fi start panics with `memory allocation
  of 4 bytes failed`. Check with `cargo tree -d | grep esp-alloc`.
- **Wi-Fi needs a scheduler.** `esp-radio` needs the `esp-rtos` scheduler, and 0.18 has only an
  async scan. The blocking examples start `esp-rtos` without its executor and poll the scan
  future from their main loop. Running the radio does not change refresh times.
- **microSD through `embedded-sdmmc` 0.7.** It has no long file names (`PaperDIY_01.png` shows
  as `PAPER~26.PNG`). It writes one 512 byte block per command, so a write runs at about
  0.17 MB/s on the test card where the Arduino sketch's FatFS path gets 0.42. Reads are faster
  than Arduino's: 0.52 against 0.40 MB/s.

## Troubleshooting

| Symptom | Likely cause | Next step |
| :--- | :--- | :--- |
| Panic `memory allocation of 4 bytes failed` when the radio starts | two `esp-alloc` versions in the build | `cargo tree -d \| grep esp-alloc`, make them one |
| `user_demo` shows the logo and the board goes dark | you held A and B for 7 seconds, which is the power-off | press PWR to start it again |

More symptoms, from bringing up a board, are in [bringup.md](bringup.md).

## Ports of the Arduino sketches

Each port draws portrait (540 x 960, `Rotation::Deg270`, connector at the bottom) except
`text_log_scroll`, which is landscape like its sketch. Fonts are `u8g2-fonts` stand-ins for
M5GFX's FreeSans, so letter shapes differ a little.

| Arduino sketch | Rust example | What it does |
| :--- | :--- | :--- |
| `Display` | `display_demo` | A page with a triple border, centered text and a bar of four gray cells, `M5Quality`. Prints `ED047TC1` where the sketch prints the board's connector name |
| `Button` | `buttons` | Shows the last button pressed, `M5Text`. Pins GPIO4 and GPIO3 |
| `TextLogScroll` | `text_log_scroll` | A scrolling log, one full-screen `M5Fastest` update per line |
| `FactoryTest` | `factory_test` | A two button menu: the checklist (battery, Wi-Fi, PSRAM, microSD, both buttons), Compare (times each refresh mode), Gray Ramp and the Game of Life |
| `PaperDIYUserDemo` | `user_demo` | The PAPER DIY poster with live widgets, all refreshed in small windows: battery voltage, a Wi-Fi list (A rescans), the microSD card with a 1 MiB write and read speed test (B runs it), A and B border feedback, and hold A and B for 2 s then 5 s to power off |

`user_demo` follows the sketch's rule that every 40th partial refresh is a full `M5Quality` one,
to clear ghosting. It does not port the sketch's factory first-boot shutdown, which needs NVS.
The sketches `EpdFastCounter`, `EpdModesCounter` and `EpdQualityRamp` are the references for the
diagnostics on the `bringup` branch (see [bringup.md](bringup.md)).

Sketches without a Rust example: `M5PM1` (PMIC timer and shutdown), `microSD` (PNG viewer),
`EpubReader` and `FastEPD/animated_smiles` (which does not work in Arduino either).

`poc_battery` reads the M5PM1 voltage registers (VBAT, VIN, 5VINOUT, VREF) once a second and
matches M5Unified's `getBatteryVoltage()`. `poc_sd` opens the card over SPI at 20 MHz, lists the
root directory and times a read. `wifi_scan` scans every 5 s and logs the access points.

## Layout

- `src/main.rs`: the `Hello M5PaperDIY!` program.
- `examples/display_demo.rs`, `buttons.rs`, `text_log_scroll.rs`, `factory_test.rs`,
  `user_demo.rs`: the ports. `factory_test` and `user_demo` keep their drawing and state
  machines in an inline `mod ui` block, which is the same text in the async twin.
- `examples/poc_*.rs`, `examples/wifi_scan.rs`: parts proven from Rust.
- `assets/`: `img_bg.png` and `img_logo.png`, M5Stack artwork copied from the Arduino sketch,
  used by `user_demo`.
- `host-tests/`: the host tests, see [bringup.md](bringup.md).
- `bringup.md`: the bring-up diagnostics and the host tests, described.
- `src/lib.rs`: pin map, `init!` (PSRAM heap), `pmic_i2c!`, `lcd_bus!`, the `pmic` module, and a
  panic handler.
- `build.rs`, `.cargo/config.toml`, `rust-toolchain.toml`: Xtensa build setup copied from
  `rust-reterminal-e1002-examples`.

## License

MIT OR Apache-2.0
