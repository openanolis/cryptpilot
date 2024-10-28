use std::time::Duration;

use again::RetryPolicy;
use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use documented::{Documented, DocumentedFields};
use kms::{plugins::aliyun::AliyunKmsClient, Annotations, Getter as _};
use log::info;
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::{IntoProvider, KeyProvider};

/// Aliyun KMS
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct KmsConfig {
    /// The id of KMS instance
    pub kms_instance_id: String,
    /// The name of the secret store in the KMS instance.
    pub secret_name: String,
    /// Content of the clientKey_****.json file.
    pub client_key: String,
    /// Content of the clientKey_****_Password.txt file.
    pub client_key_password: String,
    /// The CA cert of the KMS (the content of PrivateKmsCA_kst-******.pem file).
    pub kms_cert_pem: String,
}

pub struct KmsKeyProvider {
    options: KmsConfig,
}

impl IntoProvider for KmsConfig {
    type Provider = KmsKeyProvider;

    fn into_provider(self) -> Self::Provider {
        KmsKeyProvider { options: self }
    }
}

impl KeyProvider for KmsKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        let kms_client = AliyunKmsClient::new_client_key_client(
            &self.options.client_key,
            &self.options.kms_instance_id,
            &self.options.client_key_password,
            &self.options.kms_cert_pem,
        )?;

        // to get resource using a get_resource_provider client we do not need the Annotations.
        let max_attempts = 5;
        let key_u8 = RetryPolicy::fixed(Duration::from_secs(1))
            .with_max_retries(max_attempts - 1)
            .retry(|| async {
                kms_client
                    .get_secret(&self.options.secret_name, &Annotations::default())
                    .await
            })
            .await
            .with_context(|| {
                format!("Fail to get passphrase from KMS (attempted {max_attempts} times).")
            })?;

        let passphrase = (|| -> Result<_> {
            let key_base64 = String::from_utf8(key_u8)?;
            let key = BASE64_STANDARD.decode(&key_base64)?;
            Ok(Passphrase::from(key))
        })()
        .context("Failed to decode response from KMS as base64")?;

        info!("The passphrase has been fetched from KMS");
        return Ok(passphrase);
    }
}
