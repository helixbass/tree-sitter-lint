use std::{sync::Arc, collections::HashMap};

use derive_builder::Builder;

use crate::{Rule, configuration::Configuration};

#[non_exhaustive]
#[derive(Builder, Clone)]
#[builder(setter(into))]
pub struct Plugin {
    pub name: String,
    #[builder(default)]
    pub rules: Vec<Arc<dyn Rule>>,
    #[builder(default)]
    pub configs: HashMap<String, Configuration>,
}
