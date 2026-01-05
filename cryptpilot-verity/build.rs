use std::path::Path;

fn main() -> shadow_rs::SdResult<()> {
    shadow_rs::new()?;

    // Compile FlatBuffers schemas
    let metadata_schema = Path::new("src/metadata/metadata.fbs");
    let hash_schema = Path::new("src/metadata/metadata_hash.fbs");

    println!("cargo:rerun-if-changed={}", metadata_schema.display());
    println!("cargo:rerun-if-changed={}", hash_schema.display());

    // Get flatc binary path from flatc crate
    let flatc_path = flatc::flatc();

    let flatc_cmd = flatc_rust::Flatc::from_path(flatc_path);
    // First check with have good `flatc`
    flatc_cmd.check()?;

    // Compile main metadata schema
    flatc_cmd
        .run(flatc_rust::Args {
            inputs: &[metadata_schema],
            out_dir: Path::new("src/metadata/"),
            ..Default::default()
        })
        .expect("Failed to compile metadata.fbs");

    // Compile hash schema
    flatc_cmd
        .run(flatc_rust::Args {
            inputs: &[hash_schema],
            out_dir: Path::new("src/metadata/"),
            ..Default::default()
        })
        .expect("Failed to compile metadata_hash.fbs");

    Ok(())
}
