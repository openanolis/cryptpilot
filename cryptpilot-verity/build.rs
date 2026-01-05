use std::path::Path;

fn main() -> shadow_rs::SdResult<()> {
    shadow_rs::new()?;

    // Compile FlatBuffers schema
    let schema_path = Path::new("src/metadata/metadata.fbs");
    println!("cargo:rerun-if-changed={}", schema_path.display());

    // Get flatc binary path from flatc crate
    let flatc_path = flatc::flatc();

    let flatc_cmd = flatc_rust::Flatc::from_path(flatc_path);
    // First check with have good `flatc`
    flatc_cmd.check()?;

    flatc_cmd
        .run(flatc_rust::Args {
            inputs: &[schema_path],
            out_dir: Path::new("src/metadata/"),
            ..Default::default()
        })
        .expect("Failed to compile FlatBuffers schema");

    Ok(())
}
