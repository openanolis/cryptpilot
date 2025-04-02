use anyhow::{Result, Context};
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{types::Passphrase, fs::cmd::CheckCommandOutput as _};

use super::{IntoProvider, KeyProvider};

/// Execute Command Key Provider (reads key from command output)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct ExecConfig {
    /// Command to execute for retrieving the key
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,
}

pub struct ExecKeyProvider {
    options: ExecConfig,
}

impl IntoProvider for ExecConfig {
    type Provider = ExecKeyProvider;

    fn into_provider(self) -> Self::Provider {
        ExecKeyProvider { options: self }
    }
}

impl KeyProvider for ExecKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        let output = Command::new(&self.options.command)
            .args(&self.options.args)
            .run_check_output()
            .await
            .with_context(|| format!("Failed to execute command: {} args: {}", self.options.command, self.options.args.join(" ")))?;

        // Trim any trailing whitespace/newlines from the command output
        let mut buffer = String::from_utf8(output)
            .context("Failed to parse command output as UTF-8")?;
        
        buffer = buffer.trim_end().to_string();
        
        Ok(Passphrase::from(buffer.into_bytes()))
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
pub mod tests {
    use crate::provider::tests::{run_test_on_volume, test_volume_base};
    use crate::provider::{IntoProvider, KeyProvider};
    use crate::provider::exec::ExecConfig;

    use anyhow::Result;
    use rstest::rstest;
    use rstest_reuse::apply;
    use std::process::Stdio;
    use tokio::process::Command as TokioCommand;

    #[tokio::test]
    async fn test_get_key_from_exec() -> Result<()> {
        let config = ExecConfig {
            command: "echo".into(),
            args: vec!["test-key".into()],
        };
        
        let provider = config.into_provider();
        let key = provider.get_key().await?;
        
        assert_eq!(key.as_bytes(), b"test-key");
        
        Ok(())
    }

    #[tokio::test]
    async fn test_exec_command_failure() {
        let config = ExecConfig {
            command: "non_existent_command".into(),
            args: vec![],
        };
        
        let provider = config.into_provider();
        let result = provider.get_key().await;
        
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_trim_newlines() -> Result<()> {
        let output = TokioCommand::new("printf")
            .args(&["test-key-with-newlines\n\n"])
            .stdout(Stdio::piped())
            .output()
            .await?;
        
        let mut buffer = String::from_utf8(output.stdout)?;
        buffer = buffer.trim_end().to_string();
        
        assert_eq!(buffer, "test-key-with-newlines");
        
        Ok(())
    }

    #[apply(test_volume_base)]
    async fn test_volume(makefs: &str, integrity: bool) -> Result<()> {
        run_test_on_volume(&format!(
            r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.exec]
            command = "echo"
            args = ["test-passphrase"]
            "#,
        ))
        .await
    }
} 