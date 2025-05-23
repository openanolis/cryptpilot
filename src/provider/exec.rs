use anyhow::Result;
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{fs::cmd::CheckCommandOutput as _, types::Passphrase};

use super::KeyProvider;

/// Execute Command Key Provider (reads key from command output)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct ExecConfig {
    /// Command to execute for retrieving the key
    pub command: String,

    /// Arguments to pass to the command (optional)
    #[serde(default)]
    pub args: Vec<String>,
}

pub struct ExecKeyProvider {
    pub options: ExecConfig,
}

impl KeyProvider for ExecKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        let output = Command::new(&self.options.command)
            .args(&self.options.args)
            .run()
            .await?;

        Ok(Passphrase::from(output))
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::encrypt::KeyProviderEnum;
    use crate::provider::exec::{ExecConfig, ExecKeyProvider};
    use crate::provider::tests::{run_test_on_volume, test_volume_base};
    use crate::provider::KeyProvider;

    use anyhow::Result;
    use rstest::rstest;
    use rstest_reuse::apply;

    #[tokio::test]
    async fn test_get_key_str_from_exec() -> Result<()> {
        let config = ExecConfig {
            command: "echo".into(),
            args: vec!["-n".into(), "test-key".into()],
        };

        let provider = KeyProviderEnum::Exec(ExecKeyProvider { options: config });
        let key = provider.get_key().await?;

        assert_eq!(key.as_bytes(), b"test-key");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_key_bin_from_exec() -> Result<()> {
        let config = ExecConfig {
            command: "printf".into(),
            args: vec![r#"\x00\x01\x02\x03\n\t"#.into()],
        };

        let provider = KeyProviderEnum::Exec(ExecKeyProvider { options: config });
        let key = provider.get_key().await?;

        assert_eq!(key.as_bytes(), [0x0, 0x1, 0x2, 0x3, b'\n', b'\t']);

        Ok(())
    }

    #[tokio::test]
    async fn test_exec_command_failure() {
        let config = ExecConfig {
            command: "non_existent_command".into(),
            args: vec![],
        };

        let provider = KeyProviderEnum::Exec(ExecKeyProvider { options: config });
        let result = provider.get_key().await;

        assert!(result.is_err());
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

            [encrypt.exec]
            command = "echo"
            args = ["-n", "test-passphrase"]
            "#,
            ),
            false,
        )
        .await
    }
}
