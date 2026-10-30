# rust-m5paperdiy-embassy-examples

Async (Embassy) examples for the [`epdpi`](https://github.com/melastmohican/epdpi) e-paper
driver on the M5Stack PaperDIY (ESP32-S3, ED047TC1 panel). Structure and dependency versions follow
`rust-reterminal-e1002-examples`.

This is the async twin of `rust-m5paperdiy-examples`: every port of an Arduino sketch there has
an example of the same name here. It is a separate project on purpose: `epdpi` has mutually
exclusive blocking and async modes, and Cargo would unify them inside one workspace.

## Run

Needs the `esp` Rust toolchain and `espflash`. `epdpi` is a sibling checkout (`../epdpi`). Always
build with `--release`.

```text
cargo run --release --example user_demo
```

| Example | What it does |
| :--- | :--- |
| `epdpi_lcd_ramp` | The `LcdBus` async path: a 16-gray `M5Quality` ramp, to compare with the blocking example on the `bringup` branch of `rust-m5paperdiy-examples` |
| `display_demo` | Port of the Arduino `Display` sketch: a bordered page with text and a four cell gray bar |
| `buttons` | Port of `Button`: shows the last button pressed, `M5Text` |
| `text_log_scroll` | Port of `TextLogScroll`: a scrolling log, one full-screen `M5Fastest` update per line |
| `factory_test` | Port of `FactoryTest`: the menu with the checklist, Compare, Gray Ramp and Life Game pages |
| `user_demo` | Port of `PaperDIYUserDemo`: the poster with battery, Wi-Fi list, card speed test, button feedback and power-off |
| `pm1_timer` | Port of the `M5PM1` sketch: the power chip's timer shuts the board down and brings it back after 3 s (BtnA), or powers it off after 3 s (BtnB). It powers the board off |
| `sd_images` | Port of the `microSD` sketch: appends to a log file on the card, then shows a PNG from it; A and B flip between two images |
| `animated_smiles` | Port of FastEPD's `animated_smiles`: eight smileys bounce for 250 frames, each frame refreshing only what moved with `FastMono`, then the panel clears and powers down |
| `wifi_scan` | Logs the access points found by an `esp-radio` scan every 5 s |

Each example's header comment says what to expect. Every example says it is the async build: a small
`Rust: epdpi async` label on the screen, or `<name>: Rust, async` as the first log line for `wifi_scan`. The sketches are in
[m5paperdiy-arduino-examples](https://github.com/melastmohican/m5paperdiy-arduino-examples).

## Status

All eight examples run on a PaperDIY. Compared with the blocking twins, a refresh takes about 5
to 40 percent longer, because every row awaits the DMA instead of spinning. Measured on the same
board:

| Measured | Blocking | Async |
| :--- | :--- | :--- |
| `factory_test` Compare, full screen `M5Quality` / `M5Text` / `M5Fast` / `M5Fastest` | 955 / 633 / 396 / 265 ms | 1175 / 782 / 470 / 314 ms |
| `user_demo` border update, `M5Fastest` window | 116 ms | 156 ms |
| `user_demo` battery update, `M5Text` window | 252 ms | 352 ms |

## Things that differ from the blocking project

- Button and card polling use `embassy_time` timers. The Wi-Fi scan in `user_demo` is polled
  from the main loop with `embassy_futures::poll_once`, the same code as the blocking twin.
- The microSD card is read through `embedded-sdmmc` over a blocking SPI device in both projects,
  because the card work is short and the crate has no async API.
- `factory_test`, `user_demo`, `sd_images` and `animated_smiles` keep their drawing and state machines in an inline `mod ui`
  block. It is the same text as in `rust-m5paperdiy-examples`, where `host-tests/` tests it. Change
  one and copy the change to the other.
- Keep `esp-alloc` on the version `esp-radio` and `esp-rtos` use (0.10, check with
  `cargo tree -d | grep esp-alloc`). Two versions are two heaps, and the first Wi-Fi start then
  panics with `memory allocation of 4 bytes failed`.
- `assets/` holds M5Stack artwork copied from the Arduino sketch, used by `user_demo`.

## License

GPL-3.0-only. See [`LICENSE`](LICENSE).

The ported sketches and the artwork in `assets/` come from M5Stack's MIT-licensed
[PaperDIY UserDemo](https://github.com/m5stack) (Copyright (c) 2026 M5Stack Technology CO LTD) and keep that notice.
