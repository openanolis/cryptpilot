//! # OIDC + KMS Key Provider
//!
//! This plugin will get key in following steps
//! 1. Call the command to get an OIDC token.
//! 2. leverages One-Shot CDH to connect to get the key. This might involve
//! steps like authorization and get the key.
//!
//! Please ensure the Oneshot CDH is included in [`ONE_SHOT_CDH_BINARY_PATH`].
//! Also, ensure that the commands to get OIDC token are included in the filesystem and
//! only the OIDC token will be output to stdout.

use core::str;
use std::io::Write as _;

use anyhow::{Context as _, Result};
use base64::{
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD},
    Engine as _,
};
use documented::{Documented, DocumentedFields};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use strum::AsRefStr;
use tokio::process::Command;

use crate::{fs::cmd::CheckCommandOutput as _, types::Passphrase};

use super::KeyProvider;

const ONE_SHOT_CDH_BINARY_PATH: &str = "/usr/bin/confidential-data-hub";

/// Enum of authorization service and KMS pair
#[derive(
    Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields, AsRefStr,
)]
#[serde(tag = "type")]
pub enum Kms {
    /// Aliyun RAM + KMS
    #[serde(rename = "aliyun")]
    #[strum(serialize = "aliyun")]
    Aliyun {
        /// The ARN of the OIDC provider. This should be provided by official documents of Zero-Trust
        oidc_provider_arn: String,

        /// The ARN of the RAM Role. This should be provided by official documents of Zero-Trust
        role_arn: String,

        /// Region Id of the ECS/KMS.
        region_id: String,
    },
}

impl Kms {
    /// Export the provider settings used in Confidential Data Hub to access the
    /// KMS service. The `oidc_token` is marked as a parameter because different
    /// implementation of CDH plugin would embed the OIDC token in different fields
    /// of the settings.
    fn export_provider_settings(&self, oidc_token: String) -> Value {
        match self {
            Kms::Aliyun {
                oidc_provider_arn,
                role_arn,
                region_id,
            } => json!({
                "oidc_provider_arn": oidc_provider_arn,
                "role_arn": role_arn,
                "region_id": region_id,
                "id_token": oidc_token,
                "client_type": "oidc_ram",
            }),
        }
    }
}

/// Key Broker Service
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    /// Command to get the OIDC token
    pub command: String,

    /// Arguments to execute command to get OIDC token
    pub args: Vec<String>,

    /// The secret id in the KMS
    pub key_id: String,

    /// authorization service + kms plugin
    pub kms: Kms,
}

pub struct OidcKeyProvider {
    pub options: OidcConfig,
}

impl KeyProvider for OidcKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        #[allow(unused_variables)]
        let get_oidc_token_res = tokio::process::Command::new(&self.options.command)
            .args(&self.options.args)
            .run()
            .await
            .context("failed to execute the command to get OIDC token");
        #[cfg(not(test))]
        let oidc_token = get_oidc_token_res?;
        #[cfg(test)]
        let oidc_token = { b"test_oidc_token".to_vec() };

        let oidc_token = String::from_utf8(oidc_token).context("failed to parse OIDC token")?;

        let mut sealed_secret_file = tempfile::Builder::new()
            .prefix(".sealed_secret")
            .suffix(".json")
            .tempfile()
            .context("Failed to create temp file of sealed secret")?;

        // Here we do not set any annotations.
        // This will let the keyprovider get the latest secret in the KMS.
        let sealed_secret = json!({
            "version": "0.1.0",
            "type": "vault",
            "name": self.options.key_id,
            "provider": self.options.kms.as_ref(),
            "provider_settings": self.options.kms.export_provider_settings(oidc_token),
            "annotations": {}
        });
        let sealed_secret_json = serde_json::to_string(&sealed_secret)?;
        let sealed_secret = format!(
            "sealed.h.{}.sig",
            BASE64_URL_SAFE_NO_PAD.encode(sealed_secret_json)
        );
        sealed_secret_file
            .write_all(sealed_secret.as_bytes())
            .context("failed to write contents to sealed secret file")?;

        #[allow(unused_variables)]
        let get_secret_res = Command::new(ONE_SHOT_CDH_BINARY_PATH)
            .arg("unseal-secret")
            .arg("--secret-path")
            .arg(sealed_secret_file.path())
            .run()
            .await
            .context("failed to retrieve key using OIDC + KMS");

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
        .context("Failed to decode response from KMS with OIDC as base64")?;

        info!("The passphrase has been fetched from KMS with OIDC");
        return Ok(passphrase);
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
mod tests {
    use core::str;

    use crate::config::encrypt::KeyProviderConfig;
    use crate::provider::tests::{run_test_on_volume, test_volume_base};
    use crate::provider::{
        oidc::{Kms, OidcConfig},
        IntoProvider, KeyProvider,
    };

    use anyhow::Result;
    use rstest::rstest;
    use rstest_reuse::apply;

    #[ignore]
    #[tokio::test]
    async fn get_key() {
        let config = OidcConfig {
            command: "test/kbs/idtoken-fetcher".into(),
            args: vec![],
            key_id: "model-decryption-key".into(),
            kms: Kms::Aliyun {
                oidc_provider_arn: "acs:ram::1242424***r/cai-ecs-oidc".into(),
                role_arn: "acs:ram::124242445***ole/cai-ecs-oidc-test".into(),
                region_id: "cn-shanghai".into(),
            },
        };
        let provider = KeyProviderConfig::Oidc(config).into_provider();
        let key = provider.get_key().await.unwrap();
        println!("Get key (bytes): {:?}", key.as_bytes());
        println!("Get key (utf-8): {:?}", str::from_utf8(key.as_bytes()));
    }

    #[apply(test_volume_base)]
    async fn test_volume(makefs: &str, integrity: bool) -> Result<()> {
        run_test_on_volume(
            &format!(
                r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.oidc]
            command = "some-cli"
            args = [
                "-c",
                "/etc/config.json",
                "get-token",
            ]
            key_id = "disk-decryption-key"

            [encrypt.oidc.kms]
            type = "aliyun"
            oidc_provider_arn = "acs:ram::113511544585:oidc-provider/TestOidcIdp"
            role_arn = "acs:ram::113511544585:role/testoidc"
            region_id = "cn-beijing"
            "#,
            ),
            false,
        )
        .await
    }
}
