//! The async crate keeps its own copy of each `mod ui` block. They must be the same text as the
//! ones tested here. Skipped when the async crate is not checked out next to this one.

use std::path::Path;

fn block(source: &str) -> String {
    let marker = "mod ui {\n";
    let start = source.find(marker).expect("mod ui") + marker.len();
    let mut out = String::new();
    for line in source[start..].lines() {
        if line == "}" {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn the_async_crate_has_the_same_ui_blocks() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let there = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rust-m5paperdiy-embassy-examples/examples");
    if !there.exists() {
        eprintln!("skipped: {} does not exist", there.display());
        return;
    }
    for example in ["factory_test", "user_demo"] {
        let a = std::fs::read_to_string(here.join(format!("{example}.rs"))).unwrap();
        let b = std::fs::read_to_string(there.join(format!("{example}.rs"))).unwrap();
        assert!(
            block(&a) == block(&b),
            "{example}: the `mod ui` blocks of the two crates differ"
        );
    }
}
