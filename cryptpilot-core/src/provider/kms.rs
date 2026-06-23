use std::time::Duration;

use again::RetryPolicy;
use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use documented::{Documented, DocumentedFields};
use kms::{plugins::aliyun::AliyunKmsClient, Annotations, Getter as _};
use serde::{Deserialize, Deserializer, Serialize};

use crate::types::Passphrase;

use super::KeyProvider;

/// Aliyun KMS
#[derive(Deserialize, Serialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct KmsConfig {
    /// The id of KMS instance
    pub kms_instance_id: String,
    /// The name of the secret store in the KMS instance.
    pub secret_name: String,
    /// Authentication credentials (flattened into the same TOML table)
    #[serde(flatten, deserialize_with = "deserialize_auth_mode")]
    pub auth: AuthMode,
}

/// Authentication mode for Aliyun KMS
#[derive(Serialize, Debug, PartialEq, Clone, DocumentedFields)]
pub enum AuthMode {
    /// Client Key (application access point) authentication.
    /// Requires a pre-generated client_key JSON file and password.
    ClientKey {
        /// Content of the clientKey_****.json file.
        client_key: String,
        /// Content of the clientKey_****_Password.txt file.
        client_key_password: String,
        /// The CA cert of the KMS (the content of PrivateKmsCA_kst-******.pem file).
        kms_cert_pem: String,
    },
    /// ECS RAM Role authentication.
    /// The ECS instance must have a RAM role bound. Temporary STS credentials
    /// are fetched from the instance metadata service (IMDS) automatically.
    EcsRamRole {
        /// ECS RAM role name. Optional — if not set, discovered from IMDS.
        /// When the instance has exactly one RAM role bound, it is auto-detected.
        /// When multiple roles are bound, this field must be specified explicitly.
        ecs_ram_role_name: Option<String>,
        /// Region ID (e.g. "cn-shanghai"). Optional — auto-discovered from IMDS if not set.
        region_id: Option<String>,
    },
}

/// Intermediate deserialization helper — all fields optional
#[derive(Deserialize)]
struct AuthModeDeHelper {
    auth_mode: Option<String>,
    client_key: Option<String>,
    client_key_password: Option<String>,
    kms_cert_pem: Option<String>,
    ecs_ram_role_name: Option<String>,
    region_id: Option<String>,
}

/// Custom deserializer for AuthMode with backward compatibility:
/// - `auth_mode = "ecs_ram_role"` → EcsRamRole
/// - otherwise (including missing `auth_mode`) → ClientKey
fn deserialize_auth_mode<'de, D>(deserializer: D) -> Result<AuthMode, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let helper = AuthModeDeHelper::deserialize(deserializer)?;

    match helper.auth_mode.as_deref() {
        Some("ecs_ram_role") => Ok(AuthMode::EcsRamRole {
            ecs_ram_role_name: helper.ecs_ram_role_name,
            region_id: helper.region_id,
        }),
        _ => Ok(AuthMode::ClientKey {
            client_key: helper
                .client_key
                .ok_or_else(|| Error::custom("missing field `client_key`"))?,
            client_key_password: helper
                .client_key_password
                .ok_or_else(|| Error::custom("missing field `client_key_password`"))?,
            kms_cert_pem: helper
                .kms_cert_pem
                .ok_or_else(|| Error::custom("missing field `kms_cert_pem`"))?,
        }),
    }
}

impl KmsConfig {
    /// Create a new KmsConfig with ClientKey authentication
    #[allow(dead_code)]
    pub fn new_client_key(
        kms_instance_id: String,
        secret_name: String,
        client_key: String,
        client_key_password: String,
        kms_cert_pem: String,
    ) -> Self {
        Self {
            kms_instance_id,
            secret_name,
            auth: AuthMode::ClientKey {
                client_key,
                client_key_password,
                kms_cert_pem,
            },
        }
    }

    /// Create a new KmsConfig with ECS RAM Role authentication
    #[allow(dead_code)]
    pub fn new_ecs_ram_role(
        kms_instance_id: String,
        secret_name: String,
        ecs_ram_role_name: Option<String>,
        region_id: Option<String>,
    ) -> Self {
        Self {
            kms_instance_id,
            secret_name,
            auth: AuthMode::EcsRamRole {
                ecs_ram_role_name,
                region_id,
            },
        }
    }
}

/// IMDS metadata endpoint base URL
const IMDS_BASE: &str = "http://100.100.100.200/latest/meta-data";

/// Discover the region ID from IMDS
async fn discover_region() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client for IMDS")?;

    let region_id = client
        .get(format!("{IMDS_BASE}/region-id"))
        .send()
        .await
        .context("Failed to request region ID from IMDS")?
        .text()
        .await
        .context("Failed to read region ID from IMDS response")?;

    let region_id = region_id.trim().to_string();
    if region_id.is_empty() {
        anyhow::bail!("IMDS returned an empty region ID");
    }

    tracing::info!("Auto-discovered region ID: {}", region_id);
    Ok(region_id)
}

/// Discover the ECS RAM role name from IMDS
async fn discover_ram_role_name() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client for IMDS")?;

    let resp = client
        .get(format!("{IMDS_BASE}/ram/security-credentials/"))
        .send()
        .await
        .context("Failed to request RAM role list from IMDS")?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(
            "No RAM role bound to this ECS instance (IMDS returned 404). \
             Bind a RAM role or specify `ecs_ram_role_name` in the config."
        );
    }

    let body = resp
        .text()
        .await
        .context("Failed to read RAM role list from IMDS response")?;

    let roles: Vec<&str> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    match roles.len() {
        0 => anyhow::bail!(
            "IMDS returned an empty RAM role list. \
             Bind a RAM role to this ECS instance or specify `ecs_ram_role_name` in the config."
        ),
        1 => {
            let role_name = roles[0].to_string();
            tracing::info!("Auto-discovered RAM role name: {}", role_name);
            Ok(role_name)
        }
        _ => anyhow::bail!(
            "Multiple RAM roles bound to this ECS instance ({} found: {:?}). \
             Please specify `ecs_ram_role_name` explicitly in the config.",
            roles.len(),
            roles
        ),
    }
}

pub struct KmsKeyProvider {
    pub options: KmsConfig,
}

impl KmsKeyProvider {
    async fn get_key_from_kms(&self) -> Result<Vec<u8>> {
        let kms_client = match &self.options.auth {
            AuthMode::ClientKey {
                client_key,
                client_key_password,
                kms_cert_pem,
            } => AliyunKmsClient::new_client_key_client(
                client_key,
                &self.options.kms_instance_id,
                client_key_password,
                kms_cert_pem,
            )
            .context("Failed to create Aliyun KMS client from client key")?,

            AuthMode::EcsRamRole {
                ecs_ram_role_name,
                region_id,
            } => {
                let region_id = match region_id {
                    Some(r) => r.clone(),
                    None => discover_region().await?,
                };

                let ecs_ram_role_name = match ecs_ram_role_name {
                    Some(n) => n.clone(),
                    None => discover_ram_role_name().await?,
                };

                AliyunKmsClient::new_ecs_ram_role_client(&ecs_ram_role_name, &region_id)
            }
        };

        let max_attempts = 5;

        RetryPolicy::fixed(Duration::from_secs(1))
            .with_max_retries(max_attempts - 1)
            .retry(|| async {
                kms_client
                    .get_secret(&self.options.secret_name, &Annotations::default())
                    .await
            })
            .await
            .with_context(|| {
                format!("Fail to get passphrase from KMS (attempted {max_attempts} times).")
            })
    }
}

#[async_trait::async_trait]
impl KeyProvider for KmsKeyProvider {
    fn debug_name(&self) -> String {
        match &self.options.auth {
            AuthMode::ClientKey { .. } => {
                format!("KMS (key ID: {}) via Client Key", self.options.secret_name)
            }
            AuthMode::EcsRamRole {
                ecs_ram_role_name, ..
            } => {
                let role = ecs_ram_role_name.as_deref().unwrap_or("(auto-discovered)");
                format!(
                    "KMS (key ID: {}) via ECS RAM Role ({})",
                    self.options.secret_name, role
                )
            }
        }
    }

    async fn get_key(&self) -> Result<Passphrase> {
        let key_u8 = if cfg!(test) || std::env::var("CRYPTPILOT_TEST_MODE").is_ok() {
            BASE64_STANDARD.encode(b"test").into_bytes()
        } else {
            self.get_key_from_kms().await?
        };

        let passphrase = (|| -> Result<_> {
            let key_base64 = String::from_utf8(key_u8)?;
            let key = BASE64_STANDARD.decode(key_base64)?;
            Ok(Passphrase::from(key))
        })()
        .context("Failed to decode response from KMS as base64")?;

        tracing::info!("The passphrase has been fetched from KMS");
        Ok(passphrase)
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_key_config_deserialize() {
        let toml = r#"
            kms_instance_id = "kst-test123"
            secret_name = "test-secret"
            client_key = '{"KeyId":"KAAP.test","PrivateKeyData":"test"}'
            client_key_password = "testpass"
            kms_cert_pem = "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----\n"
        "#;
        let config: KmsConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.kms_instance_id, "kst-test123");
        assert_eq!(config.secret_name, "test-secret");
        match &config.auth {
            AuthMode::ClientKey {
                client_key,
                client_key_password,
                kms_cert_pem,
            } => {
                assert!(client_key.contains("KAAP.test"));
                assert_eq!(client_key_password, "testpass");
                assert!(kms_cert_pem.contains("BEGIN CERTIFICATE"));
            }
            AuthMode::EcsRamRole { .. } => panic!("Expected ClientKey auth mode"),
        }
    }

    #[test]
    fn test_ecs_ram_role_config_deserialize() {
        let toml = r#"
            auth_mode = "ecs_ram_role"
            kms_instance_id = "kst-test123"
            secret_name = "test-secret"
            ecs_ram_role_name = "MyRamRole"
            region_id = "cn-shanghai"
        "#;
        let config: KmsConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.kms_instance_id, "kst-test123");
        assert_eq!(config.secret_name, "test-secret");
        match &config.auth {
            AuthMode::EcsRamRole {
                ecs_ram_role_name,
                region_id,
            } => {
                assert_eq!(ecs_ram_role_name.as_deref(), Some("MyRamRole"));
                assert_eq!(region_id.as_deref(), Some("cn-shanghai"));
            }
            AuthMode::ClientKey { .. } => panic!("Expected EcsRamRole auth mode"),
        }
    }

    #[test]
    fn test_ecs_ram_role_config_deserialize_minimal() {
        let toml = r#"
            auth_mode = "ecs_ram_role"
            kms_instance_id = "kst-test123"
            secret_name = "test-secret"
        "#;
        let config: KmsConfig = toml::from_str(toml).unwrap();
        match &config.auth {
            AuthMode::EcsRamRole {
                ecs_ram_role_name,
                region_id,
            } => {
                assert!(ecs_ram_role_name.is_none());
                assert!(region_id.is_none());
            }
            _ => panic!("Expected EcsRamRole auth mode"),
        }
    }

    #[test]
    fn test_client_key_config_missing_client_key_fails() {
        let toml = r#"
            kms_instance_id = "kst-test123"
            secret_name = "test-secret"
            client_key_password = "testpass"
            kms_cert_pem = "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----\n"
        "#;
        let result: Result<KmsConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("client_key"));
    }
}
