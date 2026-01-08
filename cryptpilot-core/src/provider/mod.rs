pub mod helper;

#[cfg(feature = "provider-exec")]
pub mod exec;
#[cfg(feature = "provider-kbs")]
pub mod kbs;
#[cfg(feature = "provider-kms")]
pub mod kms;
#[cfg(feature = "provider-oidc")]
pub mod oidc;
#[cfg(feature = "provider-otp")]
pub mod otp;
#[cfg(feature = "provider-tpm2")]
pub mod tpm2;

use anyhow::Result;

use crate::types::Passphrase;

#[async_trait::async_trait]
pub trait KeyProvider {
    fn debug_name(&self) -> String;

    async fn get_key(&self) -> Result<Passphrase>;

    fn volume_type(&self) -> VolumeType;
}

pub trait IntoProvider {
    type Provider: KeyProvider;

    fn into_provider(self) -> Self::Provider;
}

pub enum VolumeType {
    /// Temporary volume, which will drop all the data after closing.
    Temporary,

    /// Persistent volume, which will keep the data after closing, and can be opened again with the same passphrase.
    Persistent,
}
