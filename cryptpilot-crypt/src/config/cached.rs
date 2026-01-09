use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::config::VolumeConfig;

use super::VolumeConfigSource;

pub struct CachedVolumeConfigSource<T: VolumeConfigSource + Sync> {
    inner: T,
    volumes: RwLock<Option<Vec<VolumeConfig>>>,
}

impl<T: VolumeConfigSource + Sync> CachedVolumeConfigSource<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            volumes: RwLock::new(None),
        }
    }
}

#[async_trait]
impl<T: VolumeConfigSource + Sync> VolumeConfigSource for CachedVolumeConfigSource<T> {
    fn source_debug_string(&self) -> String {
        self.inner.source_debug_string()
    }

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        let read = self.volumes.read().await;
        if let Some(volumes) = &*read {
            return Ok(volumes.clone());
        }
        drop(read);

        let mut write = self.volumes.write().await;
        if let Some(volumes) = &*write {
            return Ok(volumes.clone());
        }

        let volumes = self.inner.get_volume_configs().await?;
        *write = Some(volumes.clone());
        Ok(volumes)
    }
}
