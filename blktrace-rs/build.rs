use std::{env, path::PathBuf};

use bindgen::RustTarget;

fn main() {
    // Let lib.rs's compile_error! deliver the clean "Linux only" message
    // instead of a bindgen header failure on other targets.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let Ok(target_version) = RustTarget::stable(85, 0) else {
        panic!("Invalid Rust target version, at least version 1.85 required")
    };
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .rust_target(target_version)
        .derive_default(true)
        .generate()
        .expect("Unable to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
