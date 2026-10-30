# rust-m5paperdiy-examples

Board examples for `../epdpi` (crates `epdpi` and `epdpi-esp`) on the M5Stack PaperDIY (ESP32-S3, ED047TC1).
Layout mirrors `../rust-reterminal-e1002-examples`.

## Commands

```bash
cargo build --release --bins --examples
cargo clippy --bins --examples --workspace -- -D warnings
cargo fmt --all -- --check
cargo run --release --example <name>     # flash + monitor, needs the board
```

Needs the `esp` toolchain (`rustup toolchain list`). A build failure on a clean machine is
usually a missing toolchain, not code.

## Dependencies

`[dependencies]` is only for the library (`src/lib.rs`) and `src/main.rs` (Hello M5PaperDIY!).
Anything an example alone needs goes in `[dev-dependencies]`. Do not grow the main program's
dependency set for an example's sake.

## Naming

- Examples ported from [m5paperdiy-arduino-examples](https://github.com/melastmohican/m5paperdiy-arduino-examples) take no prefix
  and live in `examples/` on `main`. Every port also gets an async twin of the same name in
  `../rust-m5paperdiy-embassy-examples`. Proofs that a part works take `poc_` (or a plain name like `wifi_scan`).
- `epdpi_*` diagnostics (the bring-up programs for the `epdpi` library) are **not on `main`**. They live on the `bringup` branch,
  described in `bringup.md`. Add new diagnostics there, keep them small, and keep each one's header comment current (what it
  tests, how to run, expected result, failure meaning).

## Rules

- Every example says which build it is. With a display: a small `Rust: epdpi blocking` label (`async` in the twin), at a spot the
  page leaves free. Without one: `info!("<name>: Rust, blocking")` first in `main`. Same words in both crates.

- `src/lib.rs` holds the pin map, `init!` (PSRAM heap), `lcd_bus!`, and the M5PM1
  sequence in `pmic`. Examples stay short by using these. `display_pins!`, `Bus`, `FastBus`, `into_bus()` and
  `into_fast_bus()` are used only by the diagnostics on the `bringup` branch.
- M5PM1 is at I2C **0x6E**. The design doc's 0x6F is the M5IOE1 expander on other boards.
- Examples use the blocking `epdpi` API with `esp_hal::delay::Delay`; the bus needs
  microsecond delays.
- Use `lcd_bus!(p, 16)` (the `LCD_CAM` DMA bus) for anything that must match M5GFX's grays, `into_fast_bus()` otherwise. The portable bus (`into_bus()`) has a row
  time about 15x too long for the 16-gray waveform. See `bringup.md`.
- Power-cycle after an interrupted refresh before judging the next run.
- Never `git add .` in the parent `~/Src` repo. This directory is its own repo.
- Keep `esp-alloc` on the version `esp-radio` and `esp-rtos` use (0.10; check
  `cargo tree -d | grep esp-alloc`), or the heap that `init!` and `heap_allocator!` fill is not the
  one that allocates. `wifi_scan` starts `esp-rtos` without its Embassy executor and runs the async
  scan with `embassy_futures::block_on`, because `esp-radio` 0.18 has no blocking scan.

## Host tests

`host-tests/` is a separate crate (its own `[workspace]`, not built by the commands above) that
tests the hardware-free `mod ui` blocks of `examples/factory_test.rs` and `examples/user_demo.rs`
on the host. `build.rs` copies each block out of its example, so the tests cover the code the
board runs. They draw into a real `EpdFrameBuffer`, decode the real poster, and run the card
speed test against a RAM disk holding a FAT32 volume built in code.

```bash
host-tests/run.sh                    # all tests, from any directory
host-tests/run.sh --no-fail-fast     # see every failing file, not just the first
UI_DUMP=/some/dir host-tests/run.sh  # also writes the rendered pages as .pgm and .raw files
```

Use `run.sh`, not plain `cargo test` inside the folder: `.cargo/config.toml` here forces the ESP
target and `build-std`, and cargo reads it from the directory it starts in. `async_copy` checks
that the async crate's `mod ui` blocks are the same text, and is skipped when that crate is not
checked out next to this one. Change a `mod ui` block in one crate and you must copy it to the
other.
