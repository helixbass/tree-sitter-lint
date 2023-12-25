use std::{collections::HashMap, sync::Arc};

use derive_builder::Builder;

use crate::{configuration::Configuration, Rule};

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
