use crate::{
    cli::{BootServiceOptions, BootStage},
    cmd::Command,
};

use anyhow::Result;
use async_trait::async_trait;

pub mod auto_open;

pub struct BootServiceCommand {
    pub stage: BootStage,
}

#[async_trait]
impl Command for BootServiceCommand {
    async fn run(&self) -> Result<()> {
        match self.stage {
            BootStage::SystemVolumesAutoOpen => {
                auto_open::setup_user_provided_volumes(&BootServiceOptions {
                    stage: self.stage.clone(),
                })
                .await
            }
        }
    }
}
