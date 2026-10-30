//! Host tests for the hardware-free `mod ui` blocks of `examples/factory_test.rs`,
//! `examples/user_demo.rs` and `examples/sd_images.rs`. `build.rs` copies each block out of its example, so what is
//! tested is the code the board runs. Run with `./run.sh`.

extern crate alloc;

pub use ::epdpi;

/// What `factory_test` shows in its footer, here a fixed text.
pub const BUILD: &str = "epdpi blocking";

/// The drawing, Game of Life and button logic of `factory_test`.
pub mod factory_test {
    include!(concat!(env!("OUT_DIR"), "/factory_test_ui.rs"));
}

/// The widgets of `user_demo`: poster, battery, card, Wi-Fi and the power-off flow.
pub mod user_demo {
    include!(concat!(env!("OUT_DIR"), "/user_demo_ui.rs"));
}

/// The card access, image slots and picture of `sd_images`.
pub mod sd_images {
    include!(concat!(env!("OUT_DIR"), "/sd_images_ui.rs"));
}
