# rust-m5paperdiy-examples

> **This is the `bringup` branch.** It keeps the bring-up diagnostics (`epdpi_*`) and the host tests exactly as they were
> when `main` still carried them. `main` holds the ports of the Arduino sketches, and its `bringup.md` describes these
> diagnostics. Run them from here: `git checkout bringup`, then `cargo run --release --example epdpi_pmic_probe`.

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

It is the quickest check that a board, cable and toolchain work. If nothing appears, run the
`epdpi_*` diagnostics below in order.

## Kinds of example

| Name | Purpose | Status |
| :--- | :--- | :--- |
| `epdpi_*` | Diagnostics for the `epdpi` library. They prove each layer of the driver on real hardware. | 16 examples, all working |
| no prefix | Ports of the Arduino sketches in [m5paperdiy-arduino-examples](https://github.com/melastmohican/m5paperdiy-arduino-examples) | `display_demo`, `buttons`, `text_log_scroll`, `factory_test`, `user_demo`, all working, see [Ports](#ports-of-the-arduino-sketches) |
| `poc_*`, `wifi_scan` | Proofs that a part works from Rust, with a log and no display | `poc_battery`, `poc_sd`, `wifi_scan`, all working |

## Run

```bash
cargo run --release --example epdpi_pmic_probe      # flash and monitor via espflash
cargo run --release --example user_demo             # the PaperDIY demo
cargo build --release --bins --examples             # build everything
cargo clippy --bins --examples --workspace -- -D warnings  # CI lint
host-tests/run.sh                                   # the host tests, see below
```

Dependencies: `[dependencies]` holds only what the library and `src/main.rs` need. Anything
an example alone needs (SD card, font crates, image decoders) goes in `[dev-dependencies]`.

Requirements: the `esp` toolchain from `espup` (`rustup toolchain list` should show `esp`)
and `espflash`. Always build with `--release`: the PSRAM heap and the bus are far too slow in
a dev build. `epdpi` and `epdpi-esp` are path dependencies on the sibling checkout `../epdpi`, a workspace with both crates.

## Bring-up order (`epdpi_*`)

Run these in order. Each one proves the layer the next one depends on.

| # | Example | Proves | Success looks like |
| :-- | :--- | :--- | :--- |
| 1 | `epdpi_pmic_probe` | I2C wiring, M5PM1 present, panel rail enable | `id 0x2050`, `EPD_PWR high` |
| 2 | `epdpi_pins` | The 15 display GPIOs match the pin map | each probed pin goes high for 2 s in turn |
| 3 | `epdpi_clear` | Bus order and gate timing | panel flashes black then white, ends clean |
| 4 | `epdpi_gray_ramp` | Waveform matrix and pixel packing | 16 bars, lightening steadily left to right |
| 5 | `epdpi_text` | Framebuffer orientation, clipping | text reads normally, border on all four edges |
| 6 | `epdpi_fast_mono` | Differential updates | counter updates without a full flash, 379 ms each |

### Debug examples

Written to find the bus sequence that works. Keep them for the next board or panel.

| Example | Use |
| :--- | :--- |
| `epdpi_pmic_dump` | Read-only dump of the M5PM1 registers, to diff a working state against a cold start |
| `epdpi_polarity` | Darken-only then lighten-only through the portable bus, no waveform engine |
| `epdpi_timing` | M5GFX's line sequence on raw pins with three clock widths |
| `epdpi_row_time` | Microseconds per row on `FastBus`: bus write, scanline build, whole row |
| `epdpi_timing_bands` | One refresh with four CKV pulse lengths in horizontal bands, to see which drives best |
| `epdpi_gray_ramp_m5` | The ramp with `RefreshMode::M5Quality` (M5GFX's waveform) over `FastBus` |
| `epdpi_lcd_row_time` | Microseconds per row on `LcdBus` (22 us, M5GFX 15.5 us) |
| `epdpi_lcd_ramp` | The `M5Quality` ramp over `LcdBus`, the LCD_CAM DMA bus. Even gray steps, like M5GFX |
| `epdpi_m5_modes` | A counter with `M5Fast`, `M5Fastest` and `M5Text` in turn, five updates each, a clear between modes. Reference sketch: `EpdModesCounter` |
| `epdpi_window` | Partial-window updates: a counter in a box over a gray ramp, refreshed with `refresh_dirty`. The ramp around the box must not change |

Every example's header comment says what it tests, how to run it, what to expect, and what a
failure means.

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

## What was learned on the hardware

These cost time to find. They are recorded here so the next person does not repeat them.

- **PMIC address is 0x6E.** An earlier design note said 0x6F, which is the M5IOE1 expander
  on other M5 boards. A cold PMIC already has `PWR_CFG = 0x17`, so no rail needs enabling
  beyond GPIO2.
- **The bus line sequence matters.** A FastEPD-style sequence (short LE pulse, no wait after
  power-on) left noise on the panel. M5GFX's sequence (LE high between rows, 1 ms after
  power-on, short frame-start steps) works. `epdpi`'s `GenericGpioBus` now follows it.
  `epdpi_timing` shows that the width of the CL pulse did not matter.
- **Row time matters for grays.** The 16-gray waveform matrix assumes a row near 16 us. The
  portable bus takes about 250 us per row, which saturates the mid grays. `FastBus`
  (`epdpi-esp`, `fast_bus`) writes the data pins through the ESP32-S3 GPIO set and clear registers and
  gives a smooth ramp. It is used by `epdpi_clear`, `epdpi_gray_ramp`, `epdpi_text` and
  `epdpi_fast_mono`.
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
- **Your Arduino sketches prove the hardware.** Because M5GFX drives this panel on the same
  board, a failure with `epdpi` that M5GFX does not show is a driver bug.

## Troubleshooting

| Symptom | Likely cause | Next step |
| :--- | :--- | :--- |
| `M5PM1 did not answer` | wrong board, or I2C pins | check SDA 41, SCL 42 |
| Panel does not change at all | no drive, or FPC cable loose | `epdpi_polarity`, then reseat the FPC |
| Noise or uniform dark gray after `epdpi_clear` | wrong line sequence on the bus | `epdpi_timing` |
| Gray ramp has only two tones | row time too long (portable bus) | use `into_fast_bus()`, or `lcd_bus!` for the closest match to M5GFX |
| Mirrored or rotated text | framebuffer mapping | see `epdpi_text` |
| Old content after an interrupted run | gate drivers left in an odd state | unplug USB, rerun |
| Panic `memory allocation of 4 bytes failed` when the radio starts | two `esp-alloc` versions in the build | `cargo tree -d \| grep esp-alloc`, make them one |
| `user_demo` shows the logo and the board goes dark | you held A and B for 7 seconds, which is the power-off | press PWR to start it again |

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
The sketches `EpdFastCounter`, `EpdModesCounter` and `EpdQualityRamp` are the references for
`epdpi_fast_mono`, `epdpi_m5_modes` and `epdpi_gray_ramp_m5` / `epdpi_lcd_ramp`.

Sketches without a Rust example: `M5PM1` (PMIC timer and shutdown), `microSD` (PNG viewer),
`EpubReader` and `FastEPD/animated_smiles` (which does not work in Arduino either).

`poc_battery` reads the M5PM1 voltage registers (VBAT, VIN, 5VINOUT, VREF) once a second and
matches M5Unified's `getBatteryVoltage()`. `poc_sd` opens the card over SPI at 20 MHz, lists the
root directory and times a read. `wifi_scan` scans every 5 s and logs the access points.

## Host tests

`host-tests/` is a separate crate with 33 tests for the hardware-free code of `factory_test` and
`user_demo`: the Game of Life, button handling, the dither, the widgets' state machines, the card
speed test against a RAM disk holding a FAT32 volume, and the power-off timing. A build script
copies the `mod ui` block out of each example, so the tests run the code the board runs.

```bash
host-tests/run.sh                    # all tests
UI_DUMP=/some/dir host-tests/run.sh  # also writes the rendered pages as .pgm and .raw files
```

Use `run.sh` and not `cargo test` inside the folder: `.cargo/config.toml` here forces the ESP
target, and cargo reads it from the directory it starts in.

## Layout

- `src/main.rs`: the `Hello M5PaperDIY!` program.
- `examples/epdpi_*.rs`: library diagnostics.
- `examples/display_demo.rs`, `buttons.rs`, `text_log_scroll.rs`, `factory_test.rs`,
  `user_demo.rs`: the ports. `factory_test` and `user_demo` keep their drawing and state
  machines in an inline `mod ui` block, which is the same text in the async twin.
- `examples/poc_*.rs`, `examples/wifi_scan.rs`: parts proven from Rust.
- `assets/`: `img_bg.png` and `img_logo.png`, M5Stack artwork copied from the Arduino sketch,
  used by `user_demo`.
- `host-tests/`: the host tests.
- `src/lib.rs`: pin map, `init!` (PSRAM heap), `display_pins!`, `pmic_i2c!`, `lcd_bus!`, the
  `pmic` module, the `FastBus` type alias, and a panic handler.
- `build.rs`, `.cargo/config.toml`, `rust-toolchain.toml`: Xtensa build setup copied from
  `rust-reterminal-e1002-examples`.

## License

MIT OR Apache-2.0
