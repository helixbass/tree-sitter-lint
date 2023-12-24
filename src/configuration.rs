use derive_builder::Builder;
use serde::Deserialize;

use crate::config::{Plugins, Rules};

#[non_exhaustive]
#[derive(Builder, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[builder(default, setter(into))]
pub struct Configuration {
    pub plugins: Plugins,
    pub rules: Rules,
    pub extends: Vec<ConfigurationReference>,
}

pub type ConfigurationReference = String;
