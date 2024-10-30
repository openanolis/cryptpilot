#[cfg(feature = "provider-kbs")]
pub mod kbs;
#[cfg(feature = "provider-kms")]
pub mod kms;
#[cfg(feature = "provider-temp")]
pub mod temp;
#[cfg(feature = "provider-tpm2")]
pub mod tpm2;

use anyhow::Result;

use crate::types::Passphrase;

pub trait KeyProvider {
    async fn get_key(&self) -> Result<Passphrase>;
}

pub trait IntoProvider {
    type Provider: KeyProvider;

    fn into_provider(self) -> Self::Provider;
}
