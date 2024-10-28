use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::config::ConfigBundle;

use super::ConfigSource;

pub struct CachedConfigSource<T: ConfigSource + Sync> {
    config: RwLock<Option<ConfigBundle>>,
    inner: T,
}

impl<T: ConfigSource + Sync> CachedConfigSource<T> {
    pub fn new(inner: T) -> Self {
        Self {
            config: RwLock::new(None),
            inner,
        }
    }
}

#[async_trait]
impl<T: ConfigSource + Sync> ConfigSource for CachedConfigSource<T> {
    fn source_debug_string(&self) -> String {
        self.inner.source_debug_string()
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        let read = self.config.read().await;
        match &*read {
            None => {
                drop(read);

                let mut write: tokio::sync::RwLockWriteGuard<'_, Option<ConfigBundle>> =
                    self.config.write().await;
                // Double check
                match &*write {
                    None => {
                        let config = self.inner.get_config().await?;
                        *write = Some(config.clone());
                        Ok(config)
                    }
                    Some(v) => Ok(v.clone()),
                }
            }
            Some(v) => Ok(v.clone()),
        }
    }
}
