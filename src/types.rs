use std::fmt::Debug;

use rand::RngCore as _;
use zeroize::{Zeroize, ZeroizeOnDrop};

const GENERATED_PASSPHRASE_LEN: usize = 64;

#[derive(Zeroize, ZeroizeOnDrop, Clone)]
pub struct Passphrase(Vec<u8>);

impl Passphrase {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn random() -> Self {
        // TODO: store passphrase with auto clean container
        let mut passphrase = [0u8; GENERATED_PASSPHRASE_LEN / 2];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut passphrase);
        // Accroding to https://man7.org/linux/man-pages/man8/cryptsetup.8.html, it is highly recommended to select passphrase characters only from 7-bit ASCII.
        let passphrase = hex::encode(passphrase);

        Passphrase::from(passphrase.into_bytes())
    }
}

impl From<Vec<u8>> for Passphrase {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IntegrityType {
    None,
    Journal,
    NoJournal,
}
