use anyhow::Result;
use log::info;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::{IntoProvider, KeyProvider};

const GENERATED_PASSPHRASE_LEN: usize = 64;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct TempOptions {}

pub struct TempKeyProvider {
    options: TempOptions,
}

impl IntoProvider for TempOptions {
    type Provider = TempKeyProvider;

    fn into_provider(self) -> Self::Provider {
        TempKeyProvider { options: self }
    }
}

impl KeyProvider for TempKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        // TODO: store passphrase with auto clean container
        let mut passphrase = [0u8; GENERATED_PASSPHRASE_LEN / 2];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut passphrase);
        // Accroding to https://man7.org/linux/man-pages/man8/cryptsetup.8.html, it is highly recommended to select passphrase characters only from 7-bit ASCII.
        let passphrase = hex::encode(passphrase);

        Ok(Passphrase::from(passphrase.into_bytes()))
    }
}
