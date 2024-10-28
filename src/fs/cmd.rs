use anyhow::{bail, Result};
use async_trait::async_trait;
use tokio::process::Command;

#[async_trait]
pub trait CheckCommandOutput {
    async fn run_check_output(&mut self) -> Result<Vec<u8>>;
}

#[async_trait]
impl CheckCommandOutput for Command {
    async fn run_check_output(&mut self) -> Result<Vec<u8>> {
        self.kill_on_drop(true)
            .output()
            .await
            .map_err(anyhow::Error::from)
            .and_then(|output| {
                if !output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "\ncmd: {:?}\nexit code: {}\nstdout: {}\nstderr: {}",
                        self.as_std(),
                        match output.status.code() {
                            Some(code) => {
                                format!("{code}")
                            }
                            None => {
                                "killed by signal".into()
                            }
                        },
                        if stdout.contains('\n') {
                            format!("(multi-line)\n\t{}", stdout.replace("\n", "\n\t"))
                        } else {
                            stdout.into()
                        },
                        if stderr.contains('\n') {
                            format!("(multi-line)\n\t{}", stderr.replace("\n", "\n\t"))
                        } else {
                            stderr.into()
                        },
                    )
                } else {
                    Ok(output.stdout)
                }
            })
    }
}
