use std::fmt::Debug;

use base64::{prelude::BASE64_STANDARD, Engine};

#[derive(Clone)]
pub struct Passphrase(Vec<u8>);

impl Passphrase {
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.0.as_slice())
    }
}

impl Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(self.0.as_ref()) {
            Ok(s) => f.write_fmt(format_args!("{}", s)),
            _ => f.write_fmt(format_args!(
                "(base64) {}",
                BASE64_STANDARD.encode(self.0.as_slice())
            )),
        }
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
