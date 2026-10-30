# Bring-up diagnostics and host tests

These are the programs that proved each layer of the `epdpi` driver on an M5Stack PaperDIY, and the
host tests that cover the ports. Their code is **not** on `main`, so the examples there stay the
ports of the Arduino sketches. The diagnostics live on the
[`bringup` branch](https://github.com/melastmohican/rust-m5paperdiy-examples/tree/bringup) as they were when they left `main`, and the host
tests are in `host-tests/` on `main`.

## Running a diagnostic

Needs the `esp` toolchain from `espup` and `espflash`, like everything here. Always build with
`--release`.

```bash
git checkout bringup
cargo run --release --example epdpi_pmic_probe      # flash and monitor via espflash
```

The branch is a snapshot of `main` from before the diagnostics moved, plus a note in its README.
New diagnostics belong there. Two things in `src/lib.rs` are used only by them, so nothing on
`main` builds or runs them: the `display_pins!` macro and the `Bus`, `FastBus`, `into_bus` and
`into_fast_bus` items it feeds.

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

## Host tests

`host-tests/` is a separate crate with 63 tests for the hardware-free code of `factory_test`,
`user_demo`, `sd_images` and `animated_smiles` (the G5 image decoder and the bouncing smileys too): the Game of Life, button handling, the dither, the widgets' state machines, the card
speed test, the image search and the log file against a RAM disk holding a FAT32 volume, and the power-off timing. A build script
copies the `mod ui` block out of each example, so the tests run the code the board runs.

```bash
host-tests/run.sh                         # all tests, epdpi in blocking mode
host-tests/run.sh --no-default-features   # the same tests, epdpi in async mode
UI_DUMP=/some/dir host-tests/run.sh       # also writes the rendered pages as .pgm and .raw files
```

Use `run.sh` and not `cargo test` inside the folder: `.cargo/config.toml` here forces the ESP
target, and cargo reads it from the directory it starts in.

