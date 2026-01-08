use anyhow::Result;
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::disk::kernel::KernelArtifacts;

#[async_trait]
pub trait BootArtifacts {
    async fn inseart_reference_value<T>(
        &self,
        map: &mut IndexMap<String, Vec<String>>,
        hash_key: &str,
    ) -> Result<()>
    where
        T: digest::Digest + digest::Update;

    async fn extract_kernel_artifacts(&self) -> Result<Vec<KernelArtifacts>>;
}
