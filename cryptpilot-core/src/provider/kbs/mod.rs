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

mod ttrpc_protocol;

use ttrpc_protocol::{
    confidential_data_hub::GetResourceRequest,
    confidential_data_hub_ttrpc::GetResourceServiceClient,
};

use crate::{
    fs::cmd::CheckCommandOutput as _,
    provider::{helper, KeyProvider},
    types::Passphrase,
};

fn default_cdh_socket() -> String {
    "unix:///run/confidential-containers/cdh.sock".to_string()
}

/// The type of CDH used to get the key.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(tag = "cdh_type", rename_all = "kebab-case")]
pub enum CdhType {
    /// One-shot mode: CDH is invoked as a command-line tool.
    OneShot {
        /// The HTTP url of the KBS instance.
        kbs_url: String,
        /// The X.509 Root Cert used for HTTPS connection to the KBS instance, in PEM format. If not specified, the native Root CA certificate store in the system will be used.
        kbs_root_cert: Option<String>,
    },
    /// Daemon mode: CDH is running as a background daemon and accessible via ttrpc.
    Daemon {
        /// Optional: The socket URL for the CDH daemon.
        #[serde(default = "default_cdh_socket")]
        cdh_socket: String,
    },
}

/// Key Broker Service
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct KbsConfig {
    /// Configures the way to communicate with the confidential-data-hub: "daemon" or "one-shot" (default: "one-shot").
    #[serde(flatten, deserialize_with = "deserialize_cdh_type")]
    pub cdh_type: CdhType,

    /// The Resource URI pointing to the KBS resource used as a passphrase.
    /// Expected format: `kbs:///<repo>/<type>/<tag>`
    pub key_uri: String,
}

fn deserialize_cdh_type<'de, D>(deserializer: D) -> Result<CdhType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct RawConfig {
        cdh_type: Option<String>,
        kbs_url: Option<String>,
        kbs_root_cert: Option<String>,
        cdh_socket: Option<String>,
    }

    let raw = RawConfig::deserialize(deserializer)?;
    let cdh_type = raw.cdh_type.as_deref().unwrap_or("one-shot");

    match cdh_type {
        "one-shot" => Ok(CdhType::OneShot {
            kbs_url: raw
                .kbs_url
                .ok_or_else(|| serde::de::Error::custom("kbs_url is required for one-shot mode"))?,
            kbs_root_cert: raw.kbs_root_cert,
        }),
        "daemon" => Ok(CdhType::Daemon {
            cdh_socket: raw.cdh_socket.unwrap_or_else(default_cdh_socket),
        }),
        _ => Err(serde::de::Error::custom(format!(
            "unknown cdh_type: {}",
            cdh_type
        ))),
    }
}

pub struct KbsKeyProvider {
    pub options: KbsConfig,
}

#[async_trait::async_trait]
impl KeyProvider for KbsKeyProvider {
    fn debug_name(&self) -> String {
        let info = match &self.options.cdh_type {
            CdhType::OneShot { kbs_url, .. } => kbs_url.clone(),
            CdhType::Daemon { .. } => "via CDH daemon".to_string(),
        };
        format!("Key Broker Service ({})", info)
    }

    async fn get_key(&self) -> Result<Passphrase> {
        if cfg!(test) || std::env::var("CRYPTPILOT_TEST_MODE").is_ok() {
            return Ok(Passphrase::from(b"test".to_vec()));
        }

        let passphrase = match &self.options.cdh_type {
            CdhType::OneShot {
                kbs_url,
                kbs_root_cert,
            } => {
                let cdh_bin_path = helper::find_cdh_binary_or_default();
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

                let config = match kbs_root_cert {
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
                        kbs_url, kbs_root_cert
                    ),
                    None => format!(
                        r#"
socket = "unix:///run/confidential-containers/cdh.sock"
[kbc]
name = "cc_kbc"
url = "{}"
"#,
                        kbs_url
                    ),
                };

                cdh_config
                    .write_all(config.as_bytes())
                    .context("Failed to write contents to oneshot CDH config")?;

                let key_u8 = Command::new(cdh_bin_path)
                    .arg("-c")
                    .arg(cdh_config.path())
                    .arg("get-resource")
                    .arg("--resource-uri")
                    .arg(&self.options.key_uri)
                    .run()
                    .await
                    .with_context(|| {
                        format!("Failed to fetch passphrase from KBS URL {}", kbs_url)
                    })?;

                // The key is base64 encoded by the one-shot confidential-data-hub, so we have to decode it here.
                (|| -> Result<_> {
                    let key_base64 = String::from_utf8(key_u8)?;
                    let key_base64 = key_base64.trim_end();
                    let key = BASE64_STANDARD.decode(key_base64)?;
                    Ok(Passphrase::from(key))
                })()
                .context("Failed to decode response from KBS as base64")?
            }
            CdhType::Daemon { cdh_socket } => {
                let inner = ttrpc::r#async::Client::connect(cdh_socket).with_context(|| {
                    format!("Failed to connect to CDH ttrpc address {cdh_socket}")
                })?;
                let client = GetResourceServiceClient::new(inner);
                let request = GetResourceRequest {
                    ResourcePath: self.options.key_uri.clone(),
                    ..Default::default()
                };
                let response = client
                    .get_resource(ttrpc::context::with_timeout(5_000_000_000), &request)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to get resource {} from CDH via ttrpc",
                            self.options.key_uri
                        )
                    })?;
                Passphrase::from(response.Resource)
            }
        };

        tracing::info!("The passphrase has been fetched from KBS");
        return Ok(passphrase);
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_legacy_style() {
        let toml_legacy = r#"
            kbs_url = "https://kbs.example.com"
            key_uri = "kbs:///repo/type/tag"
        "#;
        let config: KbsConfig = toml::from_str(toml_legacy).unwrap();
        match config.cdh_type {
            CdhType::OneShot { kbs_url, .. } => assert_eq!(kbs_url, "https://kbs.example.com"),
            _ => panic!("Should be OneShot"),
        }
    }

    #[test]
    fn test_deserialize_oneshot_explicit() {
        let toml_oneshot = r#"
            cdh_type = "one-shot"
            kbs_url = "https://kbs.example.com"
            key_uri = "kbs:///repo/type/tag"
            kbs_root_cert = "PEM_DATA"
        "#;
        let config: KbsConfig = toml::from_str(toml_oneshot).unwrap();
        match config.cdh_type {
            CdhType::OneShot { kbs_root_cert, .. } => {
                assert_eq!(kbs_root_cert.unwrap(), "PEM_DATA")
            }
            _ => panic!("Should be OneShot"),
        }
    }

    #[test]
    fn test_deserialize_daemon_default() {
        let toml_daemon = r#"
            cdh_type = "daemon"
            key_uri = "kbs:///repo/type/tag"
        "#;
        let config: KbsConfig = toml::from_str(toml_daemon).unwrap();
        match config.cdh_type {
            CdhType::Daemon { cdh_socket } => {
                assert_eq!(cdh_socket, "unix:///run/confidential-containers/cdh.sock")
            }
            _ => panic!("Should be Daemon"),
        }
    }

    #[test]
    fn test_deserialize_daemon_custom() {
        let toml_daemon_custom = r#"
            cdh_type = "daemon"
            key_uri = "kbs:///repo/type/tag"
            cdh_socket = "unix:///tmp/cdh.sock"
        "#;
        let config: KbsConfig = toml::from_str(toml_daemon_custom).unwrap();
        match config.cdh_type {
            CdhType::Daemon { cdh_socket } => assert_eq!(cdh_socket, "unix:///tmp/cdh.sock"),
            _ => panic!("Should be Daemon"),
        }
    }

    #[test]
    fn test_deserialize_err_missing_url() {
        let toml_invalid = r#"
            cdh_type = "one-shot"
            key_uri = "kbs:///repo/type/tag"
        "#;
        let res: Result<KbsConfig, _> = toml::from_str(toml_invalid);
        assert!(res.is_err());
    }

    #[test]
    fn test_deserialize_err_unknown_type() {
        let toml_unknown = r#"
            cdh_type = "future-mode"
            key_uri = "kbs:///repo/type/tag"
        "#;
        let res: Result<KbsConfig, _> = toml::from_str(toml_unknown);
        assert!(res.is_err());
    }
}
