use anyhow::Result;
use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use crate::{
    provider::{
        exec::ExecKeyProvider, kbs::KbsKeyProvider, kms::KmsKeyProvider, oidc::OidcKeyProvider,
        otp::OtpKeyProvider, tpm2::Tpm2KeyProvider, IntoProvider, KeyProvider, VolumeType,
    },
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
            KeyProviderConfig::Otp(otp_config) => KeyProviderEnum::Otp(OtpKeyProvider {
                options: otp_config,
            }),
            KeyProviderConfig::Kms(kms_config) => KeyProviderEnum::Kms(KmsKeyProvider {
                options: kms_config,
            }),
            KeyProviderConfig::Kbs(kbs_config) => KeyProviderEnum::Kbs(KbsKeyProvider {
                options: kbs_config,
            }),
            KeyProviderConfig::Tpm2(tpm2_config) => KeyProviderEnum::Tpm2(Tpm2KeyProvider {
                options: tpm2_config,
            }),
            KeyProviderConfig::Oidc(oidc_config) => KeyProviderEnum::Oidc(OidcKeyProvider {
                options: oidc_config,
            }),
            KeyProviderConfig::Exec(exec_config) => KeyProviderEnum::Exec(ExecKeyProvider {
                options: exec_config,
            }),
        }
    }
}
