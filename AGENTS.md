# rust-m5paperdiy-examples

Examples for the M5Stack PaperDIY (ESP32-S3, ED047TC1) on the `epdpi` workspace, which is a checkout next to
this repository (`../epdpi`). One repository, four folders, each its own Cargo project.

- `blocking/`: the examples on the blocking API. Its `AGENTS.md` has the rules, naming and commands.
- `async/`: the async twin of every port, same names. Its `AGENTS.md` has its own rules.
- `common/`: hardware-free code shared by both. No `esp-hal`, no blocking or async code. Today it holds `epub.rs`, the EPUB
  container, package and table of contents reader, which reads through the `Storage` trait of `paperpaw-platform` (the zip and XML
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
- READMEs describe what exists now, not planned work. Planned work lives in `../epdpi/.agents/notes/backlog.md`.
- Docs read like a person wrote them: no em dashes, no "it's not X, it's Y".
- Push `epdpi` first when both change: the CI here clones `epdpi` from `main`.

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
