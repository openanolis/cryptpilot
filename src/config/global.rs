use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    /// Configuration related to cryptpilot boot service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot: Option<BootServiceConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct BootServiceConfig {
    /// Enable this option if you want to see more log when running cryptpilot boot service in initrd stage and in system stage.
    #[serde(default = "Default::default")]
    pub verbose: bool,
}

#[cfg(test)]
pub mod tests {

    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_deserialize_empty_config() -> Result<()> {
        let raw = "";

        let config: GlobalConfig = toml::from_str(raw)?;
        assert_eq!(config, GlobalConfig { boot: None });

        let raw = r#"
[boot]
        "#;
        let config: GlobalConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            GlobalConfig {
                boot: Some(BootServiceConfig { verbose: false }),
            }
        );

        let raw = r#"
[boot]
verbose = false
        "#;
        let config: GlobalConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            GlobalConfig {
                boot: Some(BootServiceConfig { verbose: false }),
            }
        );

        Ok(())
    }

    #[test]
    fn test_deserialize_wrong_config() -> Result<()> {
        let raw = r#"
        [bootddddd]
        "#;
        assert!(toml::from_str::<GlobalConfig>(raw).is_err());

        let raw = r#"
        [boot]
        [boot]
        "#;
        assert!(toml::from_str::<GlobalConfig>(raw).is_err());

        Ok(())
    }
}
