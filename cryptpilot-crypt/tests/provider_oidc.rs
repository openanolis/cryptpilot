// OIDC provider volume integration tests

mod volume_tests;

use core::str;

use anyhow::Result;
use rstest::rstest;
use rstest_reuse::apply;

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

#[apply(volume_tests::test_volume_base)]
async fn test_volume(makefs: &str, integrity: bool) -> Result<()> {
    volume_tests::run_test_on_volume(
        &format!(
            r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.oidc]
            command = "some-cli"
            args = [
                "-c",
                "/etc/config.json",
                "get-token",
            ]
            key_id = "disk-decryption-key"

            [encrypt.oidc.kms]
            type = "aliyun"
            oidc_provider_arn = "acs:ram::113511544585:oidc-provider/TestOidcIdp"
            role_arn = "acs:ram::113511544585:role/testoidc"
            region_id = "cn-beijing"
            "#,
        ),
        false,
    )
    .await
}
