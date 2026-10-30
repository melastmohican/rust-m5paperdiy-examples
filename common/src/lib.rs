//! Code shared by the blocking and the async examples, with nothing that touches the hardware, so
//! `host-tests` can run it on the host in both modes (`./run.sh` and `./run.sh --no-default-features`).
#![no_std]
extern crate alloc;

pub mod epub;
pub mod html;
pub mod layout;
pub mod reader;

/// The `Storage` trait the zip reader reads through, re-exported so the examples and the tests
/// implement it without a dependency of their own.
pub use paperpaw_platform::storage;
