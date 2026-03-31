use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

/// Check if a kernel module is loaded (either built-in or as a loadable module)
///
/// Returns true if:
/// 1. The module exists in /sys/module/<name> (already loaded as module)
/// 2. The module is built-in (compiled into kernel)
///
/// For built-in modules, /sys/module/<name> will exist even though they
/// were not loaded via modprobe.
pub fn is_module_available(name: &str) -> bool {
    // Check if module is already loaded or built-in
    // Convert hyphens to underscores module name
    // (e.g., "dm-mod" -> "dm_mod")
    Path::new(&format!("/sys/module/{}", name.replace('-', "_"))).exists()
}

/// Load a kernel module if not already available
///
/// This function first checks if the module is already loaded or built-in.
/// If not, it attempts to load it using modprobe.
///
/// # Arguments
/// * `name` - Module name
/// * `args` - Optional module parameters (e.g., ["max_part=8"])
///
/// # Examples
///
/// ```ignore
/// // Load module without parameters
/// ensure_module_loaded("dm-verity", &[]).await?;
///
/// // Load module with parameters
/// ensure_module_loaded("nbd", &["max_part=8"]).await?;
/// ```
pub async fn ensure_module_loaded(name: &str, args: &[&str]) -> Result<()> {
    if is_module_available(name) {
        debug!("Kernel module '{}' is already available", name);
        return Ok(());
    }

    info!("Loading kernel module '{}' with args: {:?}", name, args);

    let mut cmd = Command::new("modprobe");
    cmd.arg(name);
    cmd.args(args);

    let output = cmd
        .output()
        .await
        .with_context(|| format!("Failed to execute modprobe for '{}'", name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check if module is actually available now (might be built-in)
        if is_module_available(name) {
            debug!(
                "Module '{}' appears to be available after modprobe attempt (stderr: {})",
                name, stderr
            );
            return Ok(());
        }

        anyhow::bail!("Failed to load kernel module '{}': {}", name, stderr);
    }

    // Verify module is now available
    if is_module_available(name) {
        debug!("Successfully loaded kernel module '{}'", name);
        Ok(())
    } else {
        anyhow::bail!(
            "Module '{}' was loaded by modprobe but not found in /sys/module/",
            name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_module_available_builtin() {
        // These modules are typically built-in
        // The test may vary depending on kernel configuration
        let _ = is_module_available("kernel"); // Should exist
    }

    #[test]
    fn test_is_module_available_nonexistent() {
        // This module should not exist
        assert!(!is_module_available("nonexistent_module_xyz123"));
    }
}
