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

pub trait KeyProvider {
    #[allow(async_fn_in_trait)]
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

