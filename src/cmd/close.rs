use std::path::PathBuf;

use anyhow::{bail, Context as _, Result};
use log::info;
use run_script::ScriptOptions;

use crate::cli::CloseOptions;

pub async fn cmd_close(close_options: &CloseOptions) -> Result<()> {
    let volume = close_options.volume.to_owned();

    if !PathBuf::from(format!("/dev/mapper/{}", volume)).exists() {
        info!("The mapping for {} is not active, nothing to do", volume);
        return Ok(());
    }

    info!("Removing mapping for {volume}");

    let mut ops = ScriptOptions::new();
    ops.exit_on_error = true;
    run_script::run_script!(
        format!(
            r#"
            cryptsetup close {volume}
         "#
        ),
        ops
    )
    .map_err(Into::into)
    .and_then(|(code, output, error)| {
        if code != 0 {
            bail!("Bad exit code: {code}\n\tstdout: {output}\n\tstderr: {error}")
        } else {
            Ok((output, error))
        }
    })
    .with_context(|| format!("Failed to close mapping for volume `{volume}`"))?;

    info!("The mapping is removed now");

    Ok(())
}
