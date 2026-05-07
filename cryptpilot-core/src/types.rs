use std::fmt::{Debug, Display};

use rand::RngCore as _;
use serde::{Deserialize, Serialize};
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

/// Data integrity verification type for LUKS2 encrypted volumes.
///
/// Corresponds to the `--integrity` option in cryptsetup. When enabled,
/// integrity verification is performed on every I/O to prevent silent
/// data corruption.
#[derive(Debug, Clone, Copy)]
pub enum IntegrityType {
    /// Do not enable integrity verification.
    None,
    /// Enable integrity verification with journaling (default).
    ///
    /// Provides stronger data consistency guarantees and allows recovery
    /// via the journal after a crash. Suitable for persistent storage.
    /// But it will write data twice, one for journal and one for actual
    /// data area.
    Journal,
    /// Enable integrity verification without journaling.
    ///
    /// Lower write overhead compared to `Journal`, but does not support
    /// crash recovery. Suitable for performance-sensitive scenarios where
    /// some consistency risk is acceptable.
    NoJournal,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum MakeFsType {
    Swap,
    Ext4,
    Xfs,
    Vfat,
}

impl Display for MakeFsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(serde_variant::to_variant_name(self).unwrap_or("<unknown>"))
    }
}
