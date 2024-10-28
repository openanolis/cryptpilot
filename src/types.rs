use std::fmt::Debug;

use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop, Clone)]
pub struct Passphrase(Vec<u8>);

impl Passphrase {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
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
