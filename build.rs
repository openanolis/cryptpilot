use std::{env, path::PathBuf};

use bindgen::RustTarget;
#[cfg(feature = "provider-kbs")]
use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};

fn main() -> shadow_rs::SdResult<()> {
    let Ok(target_version) = RustTarget::stable(75, 0) else {
        panic!("Invalid Rust target version, at least version 1.75 required")
    };
    let bindings = bindgen::Builder::default()
        .header("src/fs/block/blktrace/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .rust_target(target_version)
        .derive_default(true)
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    #[cfg(feature = "provider-kbs")]
    {
        // Build for connecting AA with ttrpc
        let protos = vec!["src/measure/attestation_agent/protos/attestation-agent.proto"];
        let protobuf_customized = ProtobufCustomize::default().gen_mod_rs(false);

        Codegen::new()
            .out_dir("src/measure/attestation_agent/ttrpc_protocol")
            .inputs(&protos)
            .include("src/measure/attestation_agent/protos")
            .rust_protobuf()
            .customize(Customize {
                async_all: true,
                ..Default::default()
            })
            .rust_protobuf_customize(protobuf_customized)
            .run()
            .expect("Generate ttrpc protocol code failed.");
    }
    shadow_rs::new()
}
