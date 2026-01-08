#![deny(clippy::disallowed_methods)]

pub mod config;
pub mod fs;
pub mod measure;
pub mod provider;
pub mod types;
pub mod vendor;

/// A macro like scopeguard::defer! but can defer a future.
///
/// Note that other code running concurrently in the same task will be suspended
/// due to the call to block_in_place, until the future is finished.
///
/// # Examples
///
/// ```ignore
/// async_defer!(async {
///     // Do some cleanup
/// });
/// ```
///
/// # Panics
///
/// This macro should only be used in tokio multi-thread runtime, and will panics
/// if called from a [`current_thread`] runtime.
///
#[macro_export]
macro_rules! async_defer {
    ($future:expr) => {
        scopeguard::defer! {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let _ = $future.await;
                });
            });
        }
    };
}

#[cfg(test)]
mod tests {

    #[cfg(test)]
    #[ctor::ctor]
    fn init() {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "debug".into());
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}
