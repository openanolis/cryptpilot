// Library exports for cryptpilot-crypt

use shadow_rs::shadow;

shadow!(build);

pub mod cli;
pub mod cmd;
pub mod config;

// Re-export async_defer from cryptpilot core
pub use cryptpilot::async_defer;

#[doc(hidden)]
pub use scopeguard;
