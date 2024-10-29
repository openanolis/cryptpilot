use std::{
    ffi::OsStr,
    os::{linux::net::SocketAddrExt as _, unix::ffi::OsStrExt},
    path::{Component, Path},
};

use anyhow::{anyhow, bail, Context, Result};
use log::{error, info, warn};
use rand::RngCore as _;
use run_script::ScriptOptions;
use tokio::{
    io::AsyncWriteExt as _,
    net::{UnixListener, UnixStream},
    signal::unix::SignalKind,
};

use crate::{cli::CrypttabKeySupplierOptions, config, socket::SOCK_ADDR_DEFAULT};

async fn handle_request(stream: &UnixStream) -> Result<String> {
    let addr = stream
        .peer_addr()
        .context("Failed to get peer address of unix domain socket")?;

    let addr = std::os::unix::net::SocketAddr::from(addr);
    let path = addr.as_abstract_name();
    let volume_name = match path {
        Some(p) => Path::new(OsStr::from_bytes(p))
            .components()
            .skip(2)
            .next()
            .and_then(|c| {
                if let Component::Normal(name) = c {
                    name.to_str()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                anyhow!("The peer address path does not contains a valid volume name: {p:?}")
            }),
        _ => bail!("The peer address is not a invalid path: {addr:?}"),
    }?;

    let colume_configs = config::load_volume_configs().await?;
    let volume_config = colume_configs
        .iter()
        .find(|volume_config| volume_config.volume == volume_name)
        .ok_or_else(|| anyhow!("Unknown volume name: {volume_name}"))?;

    info!("Setting up for volume: {volume_name} now");

    let passphrase = match &volume_config.key_provider {
        config::KeyProviderOptions::Temp(_temp_options) => {
            let volume_config = volume_config.clone();
            let volume_name = volume_name.to_owned();

            tokio::task::spawn_blocking(move || -> Result<_> {
                let mut ops = ScriptOptions::new();
                ops.exit_on_error = true;

                // TODO: store passphrase with auto clean container
                let mut passphrase = [0u8; 16];
                let mut rng = rand::thread_rng();
                rng.fill_bytes(&mut passphrase);
                let passphrase = hex::encode(passphrase);
                info!("passphrase: {passphrase}");

                run_script::run_script!(
                    format!(
                        r#"
                        echo -n {passphrase} | cryptsetup luksFormat --type luks2 {} -
                     "#,
                        volume_config.dev
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
                .with_context(|| format!("Failed to setup LUKS2 for volume `{volume_name}`"))?;

                Ok(passphrase)
            })
            .await
            .context("background task failed")??
        }
        config::KeyProviderOptions::Kms(kms_options) => todo!(),
        config::KeyProviderOptions::Kbs(kbs_options) => todo!(),
        config::KeyProviderOptions::Tpm2(tpm2_options) => todo!(),
    };

    info!("Setting up for volume: {volume_name} finished");

    Ok(passphrase)
}

pub async fn cmd_crypttab_key_supplier(options: CrypttabKeySupplierOptions) -> Result<()> {
    let socket_path = options.socket.as_deref().unwrap_or(SOCK_ADDR_DEFAULT);

    info!("Listening on: {socket_path}");

    if Path::new(socket_path).exists() {
        tokio::fs::remove_file(socket_path).await?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("Failed to bind unix domain socket on: {socket_path}"))?;

    let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            ingress = listener.accept() => {
                match ingress {
                    Ok((mut stream, addr)) => {

                        match handle_request(&stream).await{
                            Ok(passphrase) => {
                                if let Err(e) = stream.write_all(passphrase.as_bytes()).await {
                                    error!("Error sending passphrase back to client {addr:?}: {e:#}");
                                };
                            },
                            Err(e) => {
                                error!("Error handling request from client {addr:?}: {e:#}");
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error accepting connection: {e:#}");
                    }
                }
            },
            _ = sigint.recv() => {
                break;
            },
            _ = sigterm.recv() => {
                break;
            }
        }
    }

    if let Err(e) = tokio::fs::remove_file(socket_path).await {
        warn!("Failed to clean up unix domain socket ({socket_path}): {e:#}")
    }
    info!("Received exit signal, prepare for exiting now");

    Ok(())
}
