// KBS provider volume integration tests

mod volume_tests;

use anyhow::Result;
use rstest::rstest;
use rstest_reuse::apply;

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
