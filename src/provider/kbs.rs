use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct KbsOptions {}
