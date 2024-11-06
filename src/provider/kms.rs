use anyhow::Result;
use base64::{prelude::BASE64_STANDARD, Engine as _};
use kms::{plugins::aliyun::AliyunKmsClient, Annotations, Getter as _};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::{IntoProvider, KeyProvider};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct KmsOptions {
    pub secret_name: String,
    pub client_key: String,
    pub client_key_password: String,
    pub kms_instance_id: String,
    pub kms_cert_pem: String,
}

pub struct KmsKeyProvider {
    options: KmsOptions,
}

impl IntoProvider for KmsOptions {
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
        let mut attempts = 0;
        let max_attempts = 10;
        let mut key_u8 = Vec::new();
        while attempts < max_attempts {
            match kms_client
                .get_secret(&self.options.secret_name, &Annotations::default())
                .await
            {
                Ok(resource) => {
                    key_u8 = resource;
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    warn!("Attempt {} failed: {:?}", attempts, e);

                    if attempts == max_attempts {
                        return Err(anyhow::anyhow!(
                            "Attempted {} times and all failed.",
                            max_attempts
                        ));
                    }

                    // wait before retry
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
            }
        }

        let key_base64 = String::from_utf8(key_u8)?;
        let key = BASE64_STANDARD.decode(&key_base64)?;
        let passphrase = Passphrase::from(key);

        info!("The passphrase fetched from KMS: {passphrase:?}");
        return Ok(passphrase);
    }
}
