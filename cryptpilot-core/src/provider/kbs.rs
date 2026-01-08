//! # Trustee Key Provider
//!
//! This plugin leverages One-Shot CDH to connect to a remote Trustee to get the
//! decryption key of the LUCKS volume.
//!
//! Please ensure the Oneshot CDH is included in [`ONE_SHOT_CDH_BINARY_PATH`].
//! Also, Attestation Agent must be serving in ttrpc mode in the execution environment.
use core::str;
use std::io::Write as _;

use anyhow::{Context as _, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{
    fs::cmd::CheckCommandOutput as _,
    provider::{helper, KeyProvider},
    types::Passphrase,
};

/// Key Broker Service
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct KbsConfig {
    /// The HTTP url of the KBS instance.
    pub kbs_url: String,
    /// The Resource URI in the KBS instance which refer to the KBS resource that will be used as passphrase. Should be in format `kbs:///<repo>/<type>/<tag>`
    pub key_uri: String,
    /// The X.509 Root Cert used for HTTPS connection to the KBS instance, in PEM format. If not specified, the native Root CA certificate store in the system will be used.
    pub kbs_root_cert: Option<String>,
}

pub struct KbsKeyProvider {
    pub options: KbsConfig,
}

#[async_trait::async_trait]
impl KeyProvider for KbsKeyProvider {
    fn debug_name(&self) -> String {
        format!("Key Broker Service ({})", self.options.kbs_url)
    }

    async fn get_key(&self) -> Result<Passphrase> {
        let cdh_bin_path = helper::find_cdh_binary_or_default();
        #[cfg(not(test))]
        if !std::path::Path::new(&cdh_bin_path).exists() {
            anyhow::bail!(
                "The confidential-data-hub binary not found, you may need to install it first."
            )
        }

        let mut cdh_config = tempfile::Builder::new()
            .prefix(".cdh-config")
            .suffix(".toml")
            .tempfile()
            .context("Failed to create temp file of oneshot CDH config")?;

        let config = match &self.options.kbs_root_cert {
            Some(kbs_root_cert) => format!(
                r#"
socket = "unix:///run/confidential-containers/cdh.sock"
[kbc]
name = "cc_kbc"
url = "{}"
kbs_cert = """
{}
"""
"#,
                self.options.kbs_url, kbs_root_cert
            ),
            None => format!(
                r#"
socket = "unix:///run/confidential-containers/cdh.sock"
[kbc]
name = "cc_kbc"
url = "{}"
"#,
                self.options.kbs_url
            ),
        };

        cdh_config
            .write_all(config.as_bytes())
            .context("Failed to write contents to oneshot CDH config")?;

        #[allow(unused_variables)]
        let get_secret_res = Command::new(cdh_bin_path)
            .arg("-c")
            .arg(cdh_config.path())
            .arg("get-resource")
            .arg("--resource-uri")
            .arg(&self.options.key_uri)
            .run()
            .await
            .with_context(|| {
                format!(
                    "Failed to fetch passphrase from KBS URL {}",
                    self.options.kbs_url
                )
            });

        #[cfg(not(test))]
        let key_u8 = get_secret_res?;

        #[cfg(test)]
        let key_u8 = { BASE64_STANDARD.encode(b"test").into_bytes() };

        let passphrase = (|| -> Result<_> {
            let key_base64 = String::from_utf8(key_u8)?;
            let key_base64 = key_base64.trim_end();
            let key = BASE64_STANDARD.decode(key_base64)?;
            Ok(Passphrase::from(key))
        })()
        .context("Failed to decode response from KBS as base64")?;

        tracing::info!("The passphrase has been fetched from KBS");
        return Ok(passphrase);
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}
