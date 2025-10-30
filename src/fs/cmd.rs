use std::{
    marker::{Send, Sync},
    process::Stdio,
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use tokio::{io::AsyncWriteExt as _, process::Command};

#[async_trait]
pub trait CheckCommandOutput {
    async fn run(&mut self) -> Result<Vec<u8>>;

    async fn run_with_input(&mut self, input_bytes: Option<&[u8]>) -> Result<Vec<u8>>;

    async fn run_with_status_checker<R>(
        &mut self,
        f: impl Fn(i32, Vec<u8>, Vec<u8>) -> Result<R> + Send + Sync,
    ) -> Result<R>;

    async fn run_with_input_and_status_checker<R>(
        &mut self,
        input_bytes: Option<&[u8]>,
        f: impl Fn(i32, Vec<u8>, Vec<u8>) -> Result<R> + Send + Sync,
    ) -> Result<R>;
}

#[async_trait]
impl CheckCommandOutput for Command {
    async fn run(&mut self) -> Result<Vec<u8>> {
        self.run_with_input(None).await
    }

    async fn run_with_input(&mut self, input_bytes: Option<&[u8]>) -> Result<Vec<u8>> {
        self.run_with_input_and_status_checker(input_bytes, |code, stdout, _| {
            if code != 0 {
                bail!("Bad exit code")
            } else {
                Ok(stdout)
            }
        })
        .await
    }

    async fn run_with_status_checker<R>(
        &mut self,
        f: impl Fn(i32, Vec<u8>, Vec<u8>) -> Result<R> + Send + Sync,
    ) -> Result<R> {
        self.run_with_input_and_status_checker(None, f).await
    }

    async fn run_with_input_and_status_checker<R>(
        &mut self,
        input_bytes: Option<&[u8]>,
        f: impl Fn(i32, Vec<u8>, Vec<u8>) -> Result<R> + Send + Sync,
    ) -> Result<R> {
        // reset all locale settings for this command
        self.env("LC_ALL", "C");

        tracing::trace!(cmd=?self.as_std(), "run external cmd");

        async {
            // Spawn the command and get output
            let output = async {
                if input_bytes.is_some() {
                    self.stdin(Stdio::piped());
                } else {
                    self.stdin(Stdio::null());
                }
                self.stdout(Stdio::piped());
                self.stderr(Stdio::piped());

                let mut child = self.kill_on_drop(true).spawn()?;

                if let Some(input_bytes) = input_bytes {
                    let mut stdin = child.stdin.take().context("No stdin")?;
                    stdin.write_all(input_bytes).await?;
                    stdin.shutdown().await?;
                }

                child.wait_with_output().await.map_err(anyhow::Error::from)
            }
            .await
            .with_context(|| format!("cmd: {:?}", self.as_std()))?;

            // Handle the output
            let stdout = output.stdout;
            let stderr = output.stderr;
            let code = output.status.code();

            match code {
                Some(code) => f(code, stdout.clone(), stderr.clone()),
                None => Err(anyhow!("killed by signal")),
            }
            .with_context(|| {
                let stdout = String::from_utf8_lossy(&stdout);
                let stderr = String::from_utf8_lossy(&stderr);
                format!(
                    "\ncmd: {:?}\nexit code: {}\nstdout: {}\nstderr: {}",
                    self.as_std(),
                    code.map(|code| code.to_string())
                        .unwrap_or("unknown".to_string()),
                    if stdout.contains('\n') {
                        format!("(multi-line)\n\t{}", stdout.replace('\n', "\n\t"))
                    } else {
                        stdout.into()
                    },
                    if stderr.contains('\n') {
                        format!("(multi-line)\n\t{}", stderr.replace('\n', "\n\t"))
                    } else {
                        stderr.into()
                    },
                )
            })
        }
        .await
        .context("Failed to execute external command")
    }
}
