// KBS provider volume integration tests

mod volume_tests;

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

            [encrypt.kbs]
            kbs_url = "https://1.2.3.4:8080"
            key_uri = "kbs:///default/mykey/volume_data0"
            kbs_root_cert = """
            -----BEGIN CERTIFICATE-----
            XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
            XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
            -----END CERTIFICATE-----
            """
            "#,
        ),
        false,
    )
    .await
}
