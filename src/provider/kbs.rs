//! # Trustee Key Provider
//!
//! This plugin leverages One-Shot CDH to connect to a remote Trustee to get the
//! decryption key of the LUCKS volume.
//!
//! Please ensure the Oneshot CDH is included in [`ONE_SHOT_CDH_BINARY_PATH`].
//! Also, Attestation Agent must be serving in ttrpc mode in the execution environment.
use core::str;
use std::{io::Write as _, path::Path};

use anyhow::{bail, Context as _, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use documented::{Documented, DocumentedFields};
use log::info;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{
    fs::cmd::CheckCommandOutput as _,
    provider::{IntoProvider, KeyProvider},
    types::Passphrase,
};

const ONE_SHOT_CDH_BINARY_PATH: &str = "/usr/bin/confidential-data-hub";

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
    options: KbsConfig,
}

impl IntoProvider for KbsConfig {
    type Provider = KbsKeyProvider;

    fn into_provider(self) -> Self::Provider {
        KbsKeyProvider { options: self }
    }
}

impl KeyProvider for KbsKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        if !Path::new(ONE_SHOT_CDH_BINARY_PATH).exists() {
            bail!("The confidential-data-hub binary not found, you may need to install it first.")
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
        let get_secret_res = Command::new(ONE_SHOT_CDH_BINARY_PATH)
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
            let key = BASE64_STANDARD.decode(&key_base64)?;
            Ok(Passphrase::from(key))
        })()
        .context("Failed to decode response from KBS as base64")?;

        info!("The passphrase has been fetched from KBS");
        return Ok(passphrase);
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
pub mod tests {

    use crate::provider::tests::{run_test_on_volume, test_volume_base};

    use anyhow::Result;
    use rstest::rstest;
    use rstest_reuse::apply;

    #[apply(test_volume_base)]
    async fn test_volume(makefs: &str, integrity: bool) -> Result<()> {
        run_test_on_volume(&format!(
            r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.kbs]
            kbs_url = "https://1.2.3.4:8080"
            key_uri = "kbs:///default/mykey/volume_data0"
            kbs_root_cert = """
            -----BEGIN CERTIFICATE-----
            XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
            XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
            -----END CERTIFICATE-----
            """
            "#,
        ))
        .await
    }
}
