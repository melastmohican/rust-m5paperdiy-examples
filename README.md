# rust-m5paperdiy-examples

Hardware bring-up examples for the [`epdpi`](../epdpi) driver on the **M5Stack PaperDIY**
(ESP32-S3, 4.7" ED047TC1 960 x 540 e-paper). Each file in `examples/` is a complete
`no_std` image. Run them in order, each one proves the layer the next depends on.

## Run

```bash
cargo run --release --example pmic_probe      # flash and monitor via espflash
```

Needs the `esp` toolchain (`espup`) and `espflash`. Build with `--release`: PSRAM and the
bit-banged bus are far too slow in a dev build.

## Bring-up order

| # | Example | Proves | Success looks like |
| :-- | :--- | :--- | :--- |
| 1 | `pmic_probe` | I2C wiring, M5PM1 present, panel rail enable | `id 0x2050`, `EPD_PWR high` |
| 2 | `epd_pins` | The 15 display GPIOs match the pin map | each probed pin goes high for 2 s in turn |
| 3 | `epd_clear` | Bus order and gate timing | panel flashes black then white, no stripes |
| 4 | `epd_gray_ramp` | Waveform matrix and pixel packing | 16 distinct bars, each 60 px wide |
| 5 | `epd_text` | Framebuffer orientation, clipping | text reads normally, border on all edges |
| 6 | `epd_fast_mono` | Differential updates | counter updates without a flash |

## Debug examples

Used to find the bus sequence that works. Keep them for the next board or panel.

| Example | Use |
| :--- | :--- |
| `pmic_dump` | Read-only dump of the M5PM1 registers, to diff a working state against a cold start |
| `epd_polarity` | Darken-only then lighten-only through the raw bus, no waveform engine |
| `epd_timing` | M5GFX's line sequence on raw pins with three clock widths |

## Board

| Signal | GPIO | Signal | GPIO |
| :--- | :--- | :--- | :--- |
| D0..D7 | 6, 14, 7, 12, 9, 11, 8, 10 | CKV | 18 |
| SPV | 17 | CL | 16 |
| LE | 15 | SPH | 13 |
| OE | 45 | PWR | 46 |
| I2C SDA / SCL | 41 / 42 | M5PM1 address | 0x6E |

The panel rail is gated by PMIC GPIO2, so every display example switches it on through the
M5PM1 first. The PMIC sequence and pin map come from M5GFX.

## Notes

- The ESP32-S3 module needs octal PSRAM. The two 4 bits per pixel buffers are 259,200
  bytes each and are allocated from it.
- The bus is bit-banged GPIO. A `LCD_CAM` DMA bus is a later `EpdParallelBus` implementation.
- Power-cycle the board after interrupting a refresh before judging the next run.
- `epdpi` is a path dependency on a sibling checkout (`../epdpi`).

## License

MIT OR Apache-2.0
