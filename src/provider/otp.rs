use anyhow::Result;
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::KeyProvider;

/// One Time Password (Temporary volume)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct OtpConfig {}

pub struct OtpKeyProvider {
    #[allow(dead_code)]
    pub options: OtpConfig,
}

#[async_trait::async_trait]
impl KeyProvider for OtpKeyProvider {
    fn debug_name(&self) -> String {
        "Secure Random One-Time Password".to_string()
    }

    async fn get_key(&self) -> Result<Passphrase> {
        Ok(Passphrase::random())
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Temporary
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
        run_test_on_volume(
            &format!(
                r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.otp]
            "#,
            ),
            false,
        )
        .await
    }
}
