# rust-m5paperdiy-examples

Examples for the M5Stack PaperDIY (ESP32-S3, ED047TC1) on the published crates `epdpi` and `epdpi-esp` (0.2, from
crates.io; the source is `github.com/melastmohican/epdpi`). One repository, four folders, each its own Cargo project.

- `blocking/`: the examples on the blocking API. Its `AGENTS.md` has the rules, naming and commands.
- `async/`: the async twin of every port, same names. Its `AGENTS.md` has its own rules.
- `common/`: hardware-free code shared by both. No `esp-hal`, no blocking or async code. Today it holds the EPUB reader's engine: `epub.rs` (container, package and table of
  contents), `html.rs` (XHTML to blocks) and `layout.rs` (line breaking and pages, over a `TextMetrics` trait) `reader.rs` (the book list, contents and
  reading screens and what the buttons do, over a `Library` trait) and `render.rs` (drawing them into any 4 bit gray
  `embedded-graphics` target with `u8g2` Times fonts). `UI_DUMP=/dir host-tests/run.sh --test epub_render` writes the screens as pictures. `sd.rs` is the read-only `Storage` over
  `embedded-sdmmc`, the scan for `.epub` files by long name and the `Library` the reader runs on. The card tests use a RAM disk. For a
  card with long names, make an image on a Mac and point `EPUB_FAT_IMAGE` at it:
  `hdiutil create -size 64m -fs "MS-DOS FAT32" -volname EPUBS -layout MBRSPUD card.dmg`, attach it, copy `.epub` files in,
  detach, `hdiutil convert card.dmg -format UDTO -o card`, then `EPUB_FAT_IMAGE=card.cdr host-tests/run.sh --test epub_sd -- --ignored`. `epub.rs` reads through the `Storage` trait of `paperpaw-platform` (the zip and XML
  crates `paperpaw-zip` and `paperpaw-xml` are pinned exactly, they are beta). It must build for `xtensa-esp32s3-none-elf`.
- `host-tests/`: tests for `common/` and for the `mod ui` blocks of the examples.

## Rules

- **Never one Cargo workspace** for these folders. `epdpi` is blocking or async, never both, and Cargo unifies
  features across the members of a workspace. Each folder has its own `Cargo.lock`, `target/` and
  `.cargo/config.toml`. Build from inside the folder.
- **Shared code is tested in both modes.** If `common/` depends on `epdpi`, use `default-features = false` and
  forward a `blocking` feature, as `epdpi-esp` does. `host-tests/run.sh` runs blocking, `host-tests/run.sh
  --no-default-features` runs async. Run both before pushing.
- New hardware-free code goes in `common/`. The older `mod ui` blocks in the examples are the same text in
  `blocking/` and `async/`, and the `async_copy` test fails when they differ.
- Flashing: a board with no USB serial port is off, asleep or not in download mode. Press and hold the power button on the bottom for about 2 seconds, release it when the red LED starts blinking, then run `cargo run --release --example <name>` (flash plus monitor). Check `ls /dev/cu.*` for the port before you flash, and make sure it is this board and not another ESP32.
- READMEs describe what exists now, not planned work. Planned work for this repo (the e-reader and the fonts) lives in `.agents/notes/backlog.md` here, and work on the crates lives in `../epdpi/.agents/notes/backlog.md`.
- Docs read like a person wrote them: no em dashes, no "it's not X, it's Y".
- The examples build against the published `epdpi` and `epdpi-esp`, so a change in `epdpi` shows up here only after
  a release. To try an unreleased change on the board, use the local override below. After every release of
  `epdpi` these examples are brought to the released version: step 9 of the `epdpi-release` skill in that repo.

## Trying an unreleased `epdpi` here

`epdpi` is a checkout next to this repository (`../epdpi`) when you work on it. Point the examples at it
without touching any committed file:

- `blocking/` and `async/`: create `.cargo/config.toml` in the root of this repository. It is git-ignored, and
  cargo merges it into the folders below it.

  ```toml
  [patch.crates-io]
  epdpi = { path = "../epdpi/epdpi" }
  epdpi-esp = { path = "../epdpi/epdpi-esp" }
  ```

- Host tests: `EPDPI_PATH=../epdpi host-tests/run.sh` (and with `--no-default-features`). `run.sh` starts cargo
  from a neutral directory, so the file above does not reach it.
- Check what is used: `cargo tree -p epdpi` in `blocking/` prints `epdpi v0.2.0 (/path/to/epdpi/epdpi)` with the
  override and no path without it.
- **`Cargo.lock` changes while an override is on**: cargo writes the path form of `epdpi` into it. Before you commit,
  delete `.cargo/config.toml`, run `cargo tree -p epdpi` in `blocking/` and `async/`, and check that
  `git diff --stat -- '*/Cargo.lock'` shows only changes you meant. A lockfile committed in the path form breaks
  the CI, which has no `epdpi` checkout.

## Checks (match CI)

```bash
( cd blocking && cargo build --bins --examples --release && cargo fmt --all -- --check \
    && cargo clippy --bins --examples --workspace -- -D warnings )
( cd async && cargo build --lib --examples --release && cargo fmt --all -- --check \
    && cargo clippy --lib --examples -- -D warnings )
( cd common && cargo fmt --all -- --check \
    && cargo +esp clippy --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc -- -D warnings )
host-tests/run.sh && host-tests/run.sh --no-default-features
# real books, not in the repo: EPUB_TEST_FILES="a.epub:b.epub" host-tests/run.sh --test epub_book -- --ignored
```
