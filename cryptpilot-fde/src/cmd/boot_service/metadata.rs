use std::path::Path;

use anyhow::{Context as _, Result};
use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// The version of the metadata, for future compatibility.
    pub r#type: u32,

    /// The root hash of the rootfs LV in hex format, which works with the rootfs-verity partition.
    pub root_hash: String,
}

pub async fn load_metadata_from_file(metadata_path: &Path) -> Result<Metadata> {
    let metadata_content = tokio::fs::read_to_string(&metadata_path)
        .await
        .with_context(|| format!("Can not read metadata file at {metadata_path:?}"))?;
    let mut metadata = toml::from_str::<Metadata>(&metadata_content)?;

    tracing::debug!("Metadata content:\n{}", metadata_content);

    // Sanity check on root_hash, since it is from unsafe source
    let root_hash_bin = hex::decode(metadata.root_hash).context("Bad root hash")?;
    metadata.root_hash = hex::encode(root_hash_bin);

    Ok(metadata)
}
