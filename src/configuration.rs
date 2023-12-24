use derive_builder::Builder;
use serde::Deserialize;

use crate::config::{Plugins, Rules};

#[non_exhaustive]
#[derive(Builder, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[builder(default)]
pub struct Configuration {
    pub plugins: Plugins,
    pub rules: Rules,
}
