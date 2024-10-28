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
