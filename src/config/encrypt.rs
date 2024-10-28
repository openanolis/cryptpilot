use anyhow::Result;
use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use crate::{
    provider::{IntoProvider, KeyProvider},
    types::Passphrase,
};

/// Encryption configuration for the volume.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct EncryptConfig {
    /// The key provider specific configs
    #[serde(flatten)]
    pub key_provider: KeyProviderConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum KeyProviderConfig {
    #[cfg(feature = "provider-otp")]
    Otp(crate::provider::otp::OtpConfig),
    #[cfg(feature = "provider-kms")]
    Kms(crate::provider::kms::KmsConfig),
    #[cfg(feature = "provider-kbs")]
    Kbs(crate::provider::kbs::KbsConfig),
    #[cfg(feature = "provider-tpm2")]
    Tpm2(crate::provider::tpm2::Tpm2Config),
}

pub enum KeyProviderEnum {
    #[cfg(feature = "provider-otp")]
    Otp(crate::provider::otp::OtpKeyProvider),
    #[cfg(feature = "provider-kms")]
    Kms(crate::provider::kms::KmsKeyProvider),
    #[cfg(feature = "provider-kbs")]
    Kbs(crate::provider::kbs::KbsKeyProvider),
    #[cfg(feature = "provider-tpm2")]
    Tpm2(crate::provider::tpm2::Tpm2KeyProvider),
}

impl KeyProvider for KeyProviderEnum {
    async fn get_key(&self) -> Result<Passphrase> {
        match self {
            KeyProviderEnum::Otp(provider) => provider.get_key().await,
            KeyProviderEnum::Kms(provider) => provider.get_key().await,
            KeyProviderEnum::Kbs(provider) => provider.get_key().await,
            KeyProviderEnum::Tpm2(provider) => provider.get_key().await,
        }
    }
}

impl IntoProvider for KeyProviderConfig {
    type Provider = KeyProviderEnum;

    fn into_provider(self) -> Self::Provider {
        match self {
            KeyProviderConfig::Otp(otp_config) => KeyProviderEnum::Otp(otp_config.into_provider()),
            KeyProviderConfig::Kms(kms_config) => KeyProviderEnum::Kms(kms_config.into_provider()),
            KeyProviderConfig::Kbs(kbs_config) => KeyProviderEnum::Kbs(kbs_config.into_provider()),
            KeyProviderConfig::Tpm2(tpm2_config) => {
                KeyProviderEnum::Tpm2(tpm2_config.into_provider())
            }
        }
    }
}
