use std::time::Duration;

use again::RetryPolicy;
use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use documented::{Documented, DocumentedFields};
use kms::{plugins::aliyun::AliyunKmsClient, Annotations, Getter as _};
use log::info;
use serde::{Deserialize, Serialize};

use crate::types::Passphrase;

use super::{IntoProvider, KeyProvider};

/// Aliyun KMS
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Documented, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct KmsConfig {
    /// The id of KMS instance
    pub kms_instance_id: String,
    /// The name of the secret store in the KMS instance.
    pub secret_name: String,
    /// Content of the clientKey_****.json file.
    pub client_key: String,
    /// Content of the clientKey_****_Password.txt file.
    pub client_key_password: String,
    /// The CA cert of the KMS (the content of PrivateKmsCA_kst-******.pem file).
    pub kms_cert_pem: String,
}

pub struct KmsKeyProvider {
    options: KmsConfig,
}

impl IntoProvider for KmsConfig {
    type Provider = KmsKeyProvider;

    fn into_provider(self) -> Self::Provider {
        KmsKeyProvider { options: self }
    }
}

impl KmsKeyProvider {
    #[allow(unused)]
    async fn get_key_from_kms(&self) -> Result<Vec<u8>> {
        let kms_client = AliyunKmsClient::new_client_key_client(
            &self.options.client_key,
            &self.options.kms_instance_id,
            &self.options.client_key_password,
            &self.options.kms_cert_pem,
        )?;

        // to get resource using a get_resource_provider client we do not need the Annotations.
        let max_attempts = 5;

        RetryPolicy::fixed(Duration::from_secs(1))
            .with_max_retries(max_attempts - 1)
            .retry(|| async {
                kms_client
                    .get_secret(&self.options.secret_name, &Annotations::default())
                    .await
            })
            .await
            .with_context(|| {
                format!("Fail to get passphrase from KMS (attempted {max_attempts} times).")
            })
    }
}

impl KeyProvider for KmsKeyProvider {
    async fn get_key(&self) -> Result<Passphrase> {
        #[cfg(not(test))]
        let key_u8 = self.get_key_from_kms().await?;

        #[cfg(test)]
        let key_u8 = { BASE64_STANDARD.encode(b"test").into_bytes() };

        let passphrase = (|| -> Result<_> {
            let key_base64 = String::from_utf8(key_u8)?;
            let key = BASE64_STANDARD.decode(&key_base64)?;
            Ok(Passphrase::from(key))
        })()
        .context("Failed to decode response from KMS as base64")?;

        info!("The passphrase has been fetched from KMS");
        return Ok(passphrase);
    }

    fn volume_type(&self) -> super::VolumeType {
        super::VolumeType::Persistent
    }
}

#[cfg(test)]
pub mod tests {

    use crate::provider::tests::{run_test_on_volume, test_volume_base};

    use anyhow::Result;
    use rstest::rstest;
    use rstest_reuse::apply;

    #[apply(test_volume_base)]
    async fn test_volume(makefs: &str, integrity: bool) -> Result<()> {
        run_test_on_volume(&format!(
            r#"
            volume = "<placeholder>"
            dev = "<placeholder>"
            auto_open = true
            makefs = "{makefs}"
            integrity = {integrity}

            [encrypt.kms]
            kms_instance_id = "kst-bjj66bdba95w1m0xfm3bt"
            secret_name = "luks_passphrase"
            client_key = '''
            {{
            "KeyId": "KAAP.f4c8****",
            "PrivateKeyData": "MIIJ****"
            }}'''
            client_key_password = "fa79****"
            kms_cert_pem = """
            -----BEGIN CERTIFICATE-----
            MIIDuzCCAqOgAwIBAgIJALTKwWAjvbMiMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
            BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
            BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
            ZSBLTVMgUm9vdCBDQTAeFw0yMTA3MDgwNjU1MjlaFw00MTA3MDMwNjU1MjlaMHQx
            CzAJBgNVBAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1po
            b3UxEDAOBgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwT
            UHJpdmF0ZSBLTVMgUm9vdCBDQTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoC
            ggEBAM99IVpxedcGYZVXXX4XZ+bYWw1gVD5Uli9kBrlq3nBT8c0b+4/1W4aQzr+S
            zBEWMrRZaMH3c5rV63qILyy8w4Gm2J0++nIA7uXVhpbliq6lf6p0i3cKpp+JGCbP
            kLvOpONrZ4an/htNE+vpfbsW3WcwcVbmZpQyuGIXIST8iyfTwckZSMkxAPW4rhMa
            QtmQcQiWaJsR0WJoqP7jXcHZobYehnUlzi/ZzdtmnkhTjz0+GvX9/1GBHCyfVEOO
            a0RiT5nEz55xWahZKbj+1nhmInbc7BUqfhz/mbQjtk5lAsJpA8JrbukRhTiAMbj9
            TqUqLe/meEVdjtD6wWsaZoSeoucCAwEAAaNQME4wHQYDVR0OBBYEFAVKzUR5/d6j
            nYM/bHlxURkGhe2EMB8GA1UdIwQYMBaAFAVKzUR5/d6jnYM/bHlxURkGhe2EMAwG
            A1UdEwQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAMCxpkV/KPuKVOBsT4yYkeX1
            Q7IQQoICOAkZOkg7KEej4BJpW2Ml121aFScKxdnRcV2omr48K+GQt/mPNAXgf3k2
            eKQce7xmBwntRplDJFrzdZPBdjel4i62JoWlaTejht2L5ano+x3a3keqF0GoOnn0
            StwpG2xa0S6RmyNFiMugwDBCtSCJAJKr8yAbO+hoe1lQR0M78dy8ENteC/BXuAks
            cktoG0/ryX9EqE9xQ2Do3INDq2PxIuA9yPvZ1eV3xa3bd1u+02feGIrtc9cJ5chf
            vUk5tbgg58NVXrg29yE5eq3j2BErUlAs2LB/Bt/Jhkekvp7qR42btJj+/zQnDSw=
            -----END CERTIFICATE-----
            -----BEGIN CERTIFICATE-----
            MIID3zCCAsegAwIBAgIJAO8qnQyTy8/kMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
            BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
            BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
            ZSBLTVMgUm9vdCBDQTAeFw0yMjAyMjIwNTAwMDZaFw00MjAyMTcwNTAwMDZaMIGB
            MQswCQYDVQQGEwJDTjERMA8GA1UECAwIWmhlSmlhbmcxETAPBgNVBAcMCEhhbmda
            aG91MRAwDgYDVQQKDAdBbGliYWJhMQ8wDQYDVQQLDAZBbGl5dW4xKTAnBgNVBAMM
            IFByaXZhdGUgS01TIGNuLWJlaWppbmcgU2Vjb25kIENBMIIBIjANBgkqhkiG9w0B
            AQEFAAOCAQ8AMIIBCgKCAQEAxjz6ltGz06I5BqSCabzxtvma20LcpHHKPqG3D/zb
            OS5XaOa5WOawvZUQueIXoDFnH0a/53NzfTPW8ET/0/ls7z1deirSHUmi5gUDCrit
            DdyO3bieJ0kMMinjdLGIe8hnd2H7v/X06tU+KilsEFAfFdKyVETa5iffHZRnWUlh
            NfoKAU9ycuJ2NGRE0lQ7uSB1ekCHxTNd4rsf0Oqj2xQJfR1jthf/m6rjc38/RkEM
            eI1YeADRDKxbDCmFciHs8B+q/pO+q3+o3rKhLXlu8vrJngG3tRsn/i1TQBXjAIdB
            sA2RBcni75VqATFImD9TetjwK8+oi1mdBm2WylTPm/y30QIDAQABo2YwZDAdBgNV
            HQ4EFgQUW0FY+K5NfCyUqgVjp5vH11aEUlwwHwYDVR0jBBgwFoAUBUrNRHn93qOd
            gz9seXFRGQaF7YQwEgYDVR0TAQH/BAgwBgEB/wIBADAOBgNVHQ8BAf8EBAMCAYYw
            DQYJKoZIhvcNAQELBQADggEBAI8dvj/5rTK4NxC6cNeRi4wF8HDLHLEVbOHfxQDr
            99aQmLqDL6rc9LbzRqtH8Pga606J0NsB4owyEiumYjOUyPOVyUYKrxKt5Wj/0C3V
            /sHKOdaRS+yT6O8XcsTddxbl9cIw6WroTRFvqnAtiaOt3JMCmU2rXjYa8w5tz/1t
            gTwmDuN5u4+N+zfoK0Cc2hvMJdiYFhzPYbie1ffmcHXJTNPqUg9K2lfqDCmZ+xIA
            PpVsaCU9401qPWRWftXJgb3vIVOsYB6l3KYYKdOpudaCzSbZVROmC4a693/E5hWM
            nc8BTncWI0KGWIzTQasuSEye50R6gc9wZCGIElmhWcu3NYk=
            -----END CERTIFICATE-----
            """
            "#,
        ))
        .await
    }
}
