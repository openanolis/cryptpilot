pub mod ttrpc_protocol;

use anyhow::{Context as _, Result};
use ttrpc_protocol::{
    attestation_agent::ExtendRuntimeMeasurementRequest,
    attestation_agent_ttrpc::AttestationAgentServiceClient,
};

use super::Measure;

const ATTESTATION_AGENT_TTRPC_SOCKET_DEFAULT_PATH: &str =
    "unix:///run/confidential-containers/attestation-agent/attestation-agent.sock";

const ATTESTATION_AGENT_TTRPC_TIMEOUT_NANO: i64 = 5_000_000_000;

const AAEL_DOMAIN: &str = "cryptpilot.alibabacloud.com";

pub struct AaelMeasure {
    client: AttestationAgentServiceClient,
}

impl AaelMeasure {
    pub async fn new() -> Result<Self> {
        let inner =
        ttrpc::r#async::Client::connect(ATTESTATION_AGENT_TTRPC_SOCKET_DEFAULT_PATH).with_context(|| {
            format!(
                "Failed to connect to attestation-agent ttrpc address {ATTESTATION_AGENT_TTRPC_SOCKET_DEFAULT_PATH}",
            )
        })?;
        let client = AttestationAgentServiceClient::new(inner);

        Ok(AaelMeasure { client })
    }
}

impl Measure for AaelMeasure {
    async fn extend_measurement(&self, operation: String, content: String) -> Result<()> {
        let request = ExtendRuntimeMeasurementRequest {
            Domain: AAEL_DOMAIN.into(),
            Operation: operation,
            Content: content,
            RegisterIndex: None,
            ..Default::default()
        };

        let _response = self
            .client
            .extend_runtime_measurement(
                ttrpc::context::with_timeout(ATTESTATION_AGENT_TTRPC_TIMEOUT_NANO),
                &request,
            )
            .await
            .context("Failed to extend runtime measurement")?;

        Ok(())
    }
}
