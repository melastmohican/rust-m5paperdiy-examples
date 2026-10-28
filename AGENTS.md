# rust-m5paperdiy-examples

Hardware bring-up examples for `../epdpi` on the M5Stack PaperDIY (ESP32-S3, ED047TC1).
Layout mirrors `../rust-reterminal-e1002-examples`.

## Commands

```bash
cargo build --release --examples
cargo clippy --examples --workspace -- -D warnings
cargo fmt --all -- --check
cargo run --release --example <name>     # flash + monitor, needs the board
```

Needs the `esp` toolchain (`rustup toolchain list`). A build failure on a clean machine is
usually a missing toolchain, not code.

## Rules

- `src/lib.rs` holds the pin map, `init!` (PSRAM heap), `display_pins!`, and the M5PM1
  sequence in `pmic`. Examples stay short by using these.
- M5PM1 is at I2C **0x6E**. The design doc's 0x6F is the M5IOE1 expander on other boards.
- Examples use the blocking `epdpi` API with `esp_hal::delay::Delay`; the bus needs
  microsecond delays.
- Power-cycle after an interrupted refresh before judging the next run.
- Never `git add .` in the parent `~/Src` repo. This directory is its own repo.
