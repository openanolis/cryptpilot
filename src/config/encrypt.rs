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

pub struct BoxedKeyProvider(Box<dyn KeyProvider + Send + Sync + 'static>);

#[async_trait::async_trait]
impl KeyProvider for BoxedKeyProvider {
    fn debug_name(&self) -> String {
        self.0.debug_name()
    }
    async fn get_key(&self) -> Result<Passphrase> {
        self.0.get_key().await
    }

    fn volume_type(&self) -> VolumeType {
        self.0.volume_type()
    }
}

impl IntoProvider for KeyProviderConfig {
    type Provider = BoxedKeyProvider;

    fn into_provider(self) -> Self::Provider {
        BoxedKeyProvider(match self {
            KeyProviderConfig::Otp(otp_config) => Box::new(OtpKeyProvider {
                options: otp_config,
            }),
            KeyProviderConfig::Kms(kms_config) => Box::new(KmsKeyProvider {
                options: kms_config,
            }),
            KeyProviderConfig::Kbs(kbs_config) => Box::new(KbsKeyProvider {
                options: kbs_config,
            }),
            KeyProviderConfig::Tpm2(tpm2_config) => Box::new(Tpm2KeyProvider {
                options: tpm2_config,
            }),
            KeyProviderConfig::Oidc(oidc_config) => Box::new(OidcKeyProvider {
                options: oidc_config,
            }),
            KeyProviderConfig::Exec(exec_config) => Box::new(ExecKeyProvider {
                options: exec_config,
            }),
        })
    }
}
