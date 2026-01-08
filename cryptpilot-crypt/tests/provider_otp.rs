// OTP provider volume integration tests

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

            [encrypt.otp]
            "#,
        ),
        false,
    )
    .await
}
