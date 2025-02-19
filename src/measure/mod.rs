#[cfg(feature = "provider-kbs")]
pub mod attestation_agent;

use anyhow::Result;
use serde_json::json;
use sha2::Digest;
use tracing::info;

pub const OPERATION_NAME_LOAD_CONFIG: &str = "load_config";
pub const OPERATION_NAME_FDE_ROOTFS_HASH: &str = "fde_rootfs_hash";
pub const OPERATION_NAME_INITRD_SWITCH_ROOT: &str = "initrd_switch_root";

pub trait Measure {
    #[allow(async_fn_in_trait)]
    async fn extend_measurement(&self, operation: String, content: String) -> Result<()>;

    #[allow(async_fn_in_trait)]
    async fn extend_measurement_hash(
        &self,
        operation: String,
        content_to_hash: String,
    ) -> Result<()> {
        let hash = sha2::Sha384::new()
            .chain_update(content_to_hash)
            .finalize()
            .to_vec();

        let hash = json! ({
            "alg": "sha384",
            "value": hex::encode(hash),
        });

        self.extend_measurement(operation, hash.to_string()).await
    }
}

pub enum AutoDetectMeasure {
    #[cfg(feature = "provider-kbs")]
    Aael(attestation_agent::AaelMeasure),
    Nope(NopeMeasure),
}

impl AutoDetectMeasure {
    pub async fn new() -> Self {
        #[cfg(feature = "provider-kbs")]
        {
            match attestation_agent::AaelMeasure::new()
                .await
                .map(AutoDetectMeasure::Aael)
            {
                Ok(m) => return m,
                Err(e) => {
                    info!(
                        "Failed to setup AAEL runtime measurement, disable runtime measurement now: {}",
                        e
                    )
                }
            };
        }
        AutoDetectMeasure::Nope(NopeMeasure {})
    }
}

impl Measure for AutoDetectMeasure {
    async fn extend_measurement(&self, operation: String, content: String) -> Result<()> {
        match self {
            #[cfg(feature = "provider-kbs")]
            AutoDetectMeasure::Aael(m) => m.extend_measurement(operation, content).await,
            AutoDetectMeasure::Nope(m) => m.extend_measurement(operation, content).await,
        }
    }
}

pub struct NopeMeasure {}

impl Measure for NopeMeasure {
    async fn extend_measurement(&self, _operation: String, _content: String) -> Result<()> {
        Ok(())
    }
}
