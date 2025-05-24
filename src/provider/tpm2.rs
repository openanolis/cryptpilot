use anyhow::Result;
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::KeyProvider;

/// TPM
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct Tpm2Config {}

pub struct Tpm2KeyProvider {
    #[allow(dead_code)]
    pub options: Tpm2Config,
}

#[async_trait::async_trait]
impl KeyProvider for Tpm2KeyProvider {
    fn debug_name(&self) -> String {
        "TPM".into()
    }
    async fn get_key(&self) -> Result<Passphrase> {
        todo!()
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}
