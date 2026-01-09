use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use super::{FdeConfigBundle, FdeConfigSource};

pub struct CachedFdeConfigSource<T: FdeConfigSource + Sync> {
    inner: T,
    bundle: RwLock<Option<FdeConfigBundle>>,
}

impl<T: FdeConfigSource + Sync> CachedFdeConfigSource<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            bundle: RwLock::new(None),
        }
    }
}

#[async_trait]
impl<T: FdeConfigSource + Sync> FdeConfigSource for CachedFdeConfigSource<T> {
    fn source_debug_string(&self) -> String {
        self.inner.source_debug_string()
    }

    async fn get_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        let read = self.bundle.read().await;
        if let Some(bundle) = &*read {
            return Ok(bundle.clone());
        }
        drop(read);

        let mut write = self.bundle.write().await;
        if let Some(bundle) = &*write {
            return Ok(bundle.clone());
        }

        let bundle = self.inner.get_fde_config_bundle().await?;
        *write = Some(bundle.clone());
        Ok(bundle)
    }
}
