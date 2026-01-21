use std::{env, path::PathBuf};

use bindgen::RustTarget;
#[cfg(feature = "provider-kbs")]
use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};

fn main() {
    let Ok(target_version) = RustTarget::stable(82, 0) else {
        panic!("Invalid Rust target version, at least version 1.82 required")
    };
    let bindings = bindgen::Builder::default()
        .header("src/fs/block/blktrace/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .rust_target(target_version)
        .derive_default(true)
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    #[cfg(feature = "provider-kbs")]
    {
        let protobuf_customized = ProtobufCustomize::default().gen_mod_rs(false);

        fn strip_inner_attribute(path: &std::path::Path) {
            let code = std::fs::read_to_string(path).expect("Failed to read generated file");
            let mut writer = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
            for line in code.lines() {
                if !line.starts_with("//!") && !line.starts_with("#!") {
                    std::io::Write::write_all(&mut writer, line.as_bytes()).unwrap();
                    std::io::Write::write_all(&mut writer, b"\n").unwrap();
                }
            }
        }

        {
            let aa_dir = out_dir.join("attestation-agent").join("ttrpc_protocol");
            let _ = std::fs::create_dir_all(&aa_dir); // This will panic below if the directory failed to create

            // Build for connecting AA with ttrpc
            let protos = vec!["src/measure/attestation_agent/protos/attestation-agent.proto"];

            Codegen::new()
                .out_dir(&aa_dir)
                .inputs(&protos)
                .include("src/measure/attestation_agent/protos")
                .rust_protobuf()
                .customize(Customize {
                    async_all: true,
                    ..Default::default()
                })
                .rust_protobuf_customize(protobuf_customized.clone())
                .run()
                .expect("Generate ttrpc protocol code failed.");

            strip_inner_attribute(&aa_dir.join("attestation_agent.rs"));
            strip_inner_attribute(&aa_dir.join("attestation_agent_ttrpc.rs"));
        }

        {
            let aa_dir = out_dir.join("confidential-data-hub").join("ttrpc_protocol");
            let _ = std::fs::create_dir_all(&aa_dir); // This will panic below if the directory failed to create

            // Build for connecting AA with ttrpc
            let protos = vec!["src/provider/kbs/protos/confidential-data-hub.proto"];

            Codegen::new()
                .out_dir(&aa_dir)
                .inputs(&protos)
                .include("src/provider/kbs/protos")
                .rust_protobuf()
                .customize(Customize {
                    async_all: true,
                    ..Default::default()
                })
                .rust_protobuf_customize(protobuf_customized.clone())
                .run()
                .expect("Generate ttrpc protocol code failed.");

            strip_inner_attribute(&aa_dir.join("confidential_data_hub.rs"));
            strip_inner_attribute(&aa_dir.join("confidential_data_hub_ttrpc.rs"));
        }
    }
}
