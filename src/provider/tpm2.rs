use anyhow::Result;
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::{IntoProvider, KeyProvider};

/// TPM
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct Tpm2Config {}

pub struct Tpm2KeyProvider {
    #[allow(dead_code)]
    options: Tpm2Config,
}

impl IntoProvider for Tpm2Config {
    type Provider = Tpm2KeyProvider;

    fn into_provider(self) -> Self::Provider {
        Tpm2KeyProvider { options: self }
    }
}

impl KeyProvider for Tpm2KeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        todo!()
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}
