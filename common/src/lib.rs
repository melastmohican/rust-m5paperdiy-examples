//! Code shared by the blocking and the async examples, with nothing that touches the hardware, so
//! `host-tests` can run it on the host in both modes (`./run.sh` and `./run.sh --no-default-features`).
#![no_std]
extern crate alloc;
