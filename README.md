# rust-m5paperdiy-examples

Rust examples for the **M5Stack PaperDIY** (ESP32-S3, 4.7" ED047TC1 960 x 540 e-paper,
16 grays, two buttons, microSD, M5PM1 power chip), built on the
[`epdpi`](../epdpi) driver. Each file in `examples/` is a complete `no_std` image.

## Hello M5PaperDIY!

`src/main.rs` is the smallest real program: it shows "Hello M5PaperDIY!" in the middle of the
screen and stops.

```bash
cargo run --release
```

It is the quickest check that a board, cable and toolchain work. If nothing appears, run the
`epdpi_*` diagnostics below in order.

## Two kinds of example

| Prefix | Purpose | Status |
| :--- | :--- | :--- |
| `epdpi_*` | Basic diagnostics for the `epdpi` library. They prove each layer of the driver on real hardware. | 14 examples, all working |
| no prefix | Real board examples, ports of the Arduino sketches in `~/Src/Arduino/M5PaperDIY` | planned, see [Porting plan](#porting-plan) |

## Run

```bash
cargo run --release --example epdpi_pmic_probe      # flash and monitor via espflash
cargo build --release --bins --examples             # build everything
cargo clippy --bins --examples --workspace -- -D warnings  # CI lint
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

The panel rail is gated twice: PMIC GPIO2 (EPD_PWR) over I2C and ESP32 GPIO46. Every display
example switches the PMIC on first through `pmic::epd_power_on`. Pin map and PMIC sequence
come from M5GFX (`board_M5PaperDIY`). The button and microSD pins come from M5Unified and
the Arduino sketches, and no Rust example has used them yet.

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
  has 8 MB of octal PSRAM. `esp-alloc` 0.9's `psram_allocator!` does not match the esp-hal
  1.1 PSRAM API, so the `init!` macro registers the region by hand.
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

## Porting plan

Which Arduino sketches in `~/Src/Arduino/M5PaperDIY` can become real board examples. Effort
is a rough size for one example, assuming the driver as it is today.

| Sketch | Port as | Needs | Effort | Notes |
| :--- | :--- | :--- | :--- | :--- |
| Button | `buttons` | GPIO4 / GPIO3 inputs, `FastMono` | small | Shows the last press on screen. Good first real example. |
| Display | `display_demo` | embedded-graphics, a larger font crate | small | M5GFX's FreeSans fonts have no direct match. `profont` or `u8g2-fonts` would do. |
| TextLogScroll | `text_log_scroll` | `FastMono`, a 1-bit text buffer | small | A fast-update stress test. |
| FactoryTest: gray ramp | already done | none | none | Same as `epdpi_gray_ramp`. |
| FactoryTest: refresh compare | `refresh_compare` | the three `RefreshMode`s | small | M5GFX has four modes, `epdpi` has three. |
| FactoryTest: Life game | `life` | `FastMono` | small | Pure compute plus updates. |
| M5PM1 | `pm1_timer` | PMIC timer registers over I2C | medium | `shutdown` powers the board off. Test with care. |
| Battery readout (from UserDemo and FactoryTest) | `battery` | PMIC VBAT registers 0x22 and 0x23 | small | Already readable, see `epdpi_pmic_dump`. |
| microSD | `sd_images` | `embedded-sdmmc`, a PNG or BMP decoder | medium | Convert the PNGs to BMP to skip a decoder. |
| PaperDIYUserDemo: TF card speed test | `sd_speed` | `embedded-sdmmc` | medium | |
| PaperDIYUserDemo: Wi-Fi scan | `wifi_scan` | `esp-radio` and an executor | large | New toolchain surface. Do last. |
| PaperDIYUserDemo: region refresh | n/a | partial-window refresh in `epdpi` | large | The driver refreshes whole frames only. |
| EpubReader | n/a | zip, XML, fonts, SD | very large | Not a port, a new project. |
| FastEPD/animated_smiles | n/a | | | Does not work in Arduino either. |

Driver gaps that limit these ports: no partial-window refresh, no rotation, only the
`embedded-graphics` mono fonts, and no `LCD_CAM` DMA bus (so the CPU is busy during every
refresh).

## Layout

- `src/main.rs`: the `Hello M5PaperDIY!` program.
- `examples/epdpi_*.rs`: library diagnostics.
- `src/lib.rs`: pin map, `init!` (PSRAM heap), `display_pins!`, `pmic_i2c!`, the `pmic`
  module, the `FastBus` type alias, and a panic handler.
- `build.rs`, `.cargo/config.toml`, `rust-toolchain.toml`: Xtensa build setup copied from
  `rust-reterminal-e1002-examples`.

## License

MIT OR Apache-2.0
