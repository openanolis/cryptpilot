// Volume mkfs with integrity tests

use std::path::PathBuf;

use cryptpilot_crypt::{
    async_defer,
    cli::{CloseOptions, OpenOptions},
    cmd::{close::CloseCommand, open::OpenCommand, Command as _},
    config::{
        source::{set_volume_config_source, VolumeConfigSource},
        volume::{ExtraConfig, VolumeConfig},
    },
};

use cryptpilot::{
    config::encrypt::{EncryptConfig, KeyProviderConfig},
    fs::{block::dummy::DummyDevice, cmd::CheckCommandOutput as _},
    provider::otp::OtpConfig,
    types::MakeFsType,
};

use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command;

#[cfg(test)]
#[ctor::ctor]
fn init() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "debug".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

struct InMemoryVolumeConfigSource {
    volumes: Vec<VolumeConfig>,
}

#[async_trait]
impl VolumeConfigSource for InMemoryVolumeConfigSource {
    fn source_debug_string(&self) -> String {
        "in-memory test volume config".to_owned()
    }

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        Ok(self.volumes.clone())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_mkfs_with_integrity() -> Result<()> {
    // Set test mode environment variable to skip some external binary checks
    std::env::set_var("CRYPTPILOT_TEST_MODE", "1");

    let dummy_device = DummyDevice::setup_on_tmpfs(10 * 1024 * 1024 * 1024).await?;

    let volume_config = VolumeConfig {
        volume: "mkfs_with_integrity".to_owned(),
        dev: dummy_device.path().unwrap(),
        extra_config: ExtraConfig {
            auto_open: Some(true),
            makefs: Some(MakeFsType::Ext4),
            integrity: Some(true),
        },
        encrypt: EncryptConfig {
            key_provider: KeyProviderConfig::Otp(OtpConfig {}),
        },
    };

    set_volume_config_source(InMemoryVolumeConfigSource {
        volumes: vec![volume_config.clone()],
    })
    .await;

    // Close the volume if it is already opened
    CloseCommand {
        close_options: CloseOptions {
            volume: vec![volume_config.volume.clone()],
        },
    }
    .run()
    .await
    .unwrap();

    async_defer! {
        async{
            CloseCommand {
                close_options: CloseOptions {
                    volume: vec![volume_config.volume.clone()],
                }
            }.run().await.unwrap();
        }
    }

    OpenCommand {
        open_options: OpenOptions {
            volume: vec![volume_config.volume.clone()],
            check_fs: false,
        },
    }
    .run()
    .await?;

    Command::new("blkid")
        .arg("-p")
        .arg(PathBuf::from("/dev/mapper/").join(&volume_config.volume))
        .run()
        .await?;

    Ok(())
}
