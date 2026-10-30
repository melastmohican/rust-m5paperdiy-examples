# rust-m5paperdiy-examples

Rust examples for the **M5Stack PaperDIY** (ESP32-S3, 4.7" ED047TC1 960 x 540 e-paper, 16 grays, two
buttons, microSD, M5PM1 power chip), built on the [`epdpi`](https://github.com/melastmohican/epdpi)
driver. The same examples exist twice, once for the blocking API of `epdpi` and once for its async
API on Embassy.

| Folder | What is in it |
| :--- | :--- |
| [`blocking/`](blocking) | The blocking examples: ports of the Arduino sketches in [m5paperdiy-arduino-examples](https://github.com/melastmohican/m5paperdiy-arduino-examples), proofs that a part works, board support. Start with its [README](blocking/README.md). |
| [`async/`](async) | The async (Embassy) twin of every port, same names. See its [README](async/README.md). |
| [`common/`](common) | Hardware-free code shared by both, tested on the host in both modes. Today: the EPUB container, package and table of contents reader (`common/src/epub.rs`). |
| [`host-tests/`](host-tests) | Host tests for the hardware-free code, run in blocking and in async mode. |

The `bringup` branch holds the bring-up diagnostics for the `epdpi` library, described in
[bringup.md](bringup.md).

## Run

Needs the `esp` Rust toolchain (`espup`), `espflash`, and a checkout of
[`epdpi`](https://github.com/melastmohican/epdpi) next to this repository (`../epdpi`). Build and run
from the folder of the build you want, always with `--release`:

```text
cd blocking && cargo run --release --example display_demo
cd async    && cargo run --release --example display_demo
```

## Tests

```text
host-tests/run.sh                         # epdpi in blocking mode
host-tests/run.sh --no-default-features   # the same tests, epdpi in async mode
```

`blocking/`, `async/`, `common/` and `host-tests/` are separate Cargo projects on purpose. `epdpi`
has mutually exclusive blocking and async modes, and one Cargo workspace would unify them.

## License

GPL-3.0-only. See [`LICENSE`](LICENSE).

The ported sketches and the artwork in `blocking/assets/` and `async/assets/` come from M5Stack's
MIT-licensed [PaperDIY UserDemo](https://github.com/m5stack/M5PaperDIY-UserDemo) (Copyright (c) 2026 M5Stack Technology CO LTD) and keep that notice.
