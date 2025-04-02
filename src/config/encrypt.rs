use anyhow::Result;
use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use crate::{
    provider::{IntoProvider, KeyProvider, VolumeType},
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
    #[cfg(feature = "provider-oidc")]
    Oidc(crate::provider::oidc::OidcConfig),
    #[cfg(feature = "provider-exec")]
    Exec(crate::provider::exec::ExecConfig),
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
    #[cfg(feature = "provider-oidc")]
    Oidc(crate::provider::oidc::OidcKeyProvider),
    #[cfg(feature = "provider-exec")]
    Exec(crate::provider::exec::ExecKeyProvider),
}

impl KeyProvider for KeyProviderEnum {
    async fn get_key(&self) -> Result<Passphrase> {
        match self {
            KeyProviderEnum::Otp(provider) => provider.get_key().await,
            KeyProviderEnum::Kms(provider) => provider.get_key().await,
            KeyProviderEnum::Kbs(provider) => provider.get_key().await,
            KeyProviderEnum::Tpm2(provider) => provider.get_key().await,
            KeyProviderEnum::Oidc(provider) => provider.get_key().await,
            KeyProviderEnum::Exec(provider) => provider.get_key().await,
        }
    }

    fn volume_type(&self) -> VolumeType {
        match self {
            KeyProviderEnum::Otp(provider) => provider.volume_type(),
            KeyProviderEnum::Kms(provider) => provider.volume_type(),
            KeyProviderEnum::Kbs(provider) => provider.volume_type(),
            KeyProviderEnum::Tpm2(provider) => provider.volume_type(),
            KeyProviderEnum::Oidc(provider) => provider.volume_type(),
            KeyProviderEnum::Exec(provider) => provider.volume_type(),
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
            KeyProviderConfig::Oidc(oidc_config) => {
                KeyProviderEnum::Oidc(oidc_config.into_provider())
            }
            KeyProviderConfig::Exec(exec_config) => {
                KeyProviderEnum::Exec(exec_config.into_provider())
            }
        }
    }
}
