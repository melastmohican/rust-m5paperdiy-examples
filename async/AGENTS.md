# rust-m5paperdiy-embassy-examples

Async twin of `../rust-m5paperdiy-examples`, for the `epdpi` workspace in `../epdpi`. Model it on
`../rust-reterminal-e1002-examples`.

- Own project, never a member of the blocking examples workspace (feature unification would turn
  `epdpi/blocking` back on). Board support in `src/lib.rs` is copied on purpose.
- Build and run from here: `cargo build --release --examples`, `cargo run --release --example X`.
- Checks: `cargo fmt --all -- --check`, `cargo clippy --release --examples --lib -- -D warnings`.
- Every example says it is the async build: a small `Rust: epdpi async` label on the screen, or `info!("<name>: Rust, async")` first in
  `main` when it has no display. Same words as the blocking twin.
- Blocking is only for start-up work (PMIC I2C). Everything on the bus is `.await`ed.
- Open work lives in `../epdpi/.agents/notes/backlog.md`, queue item 2 and 3.
- Keep `esp-alloc` on the same version as `esp-radio` and `esp-rtos` use (check
  `cargo tree -d | grep esp-alloc`). Two copies mean two heaps: `heap_allocator!` fills ours, the
  global allocator is the other one and is empty, and the first allocation in `esp_radio::wifi::new`
  panics with `memory allocation of 4 bytes failed`.
- `esp-radio` needs the `esp-rtos` scheduler (`start_rtos!`) and has no blocking scan in 0.18, so
  Wi-Fi examples exist here only.
