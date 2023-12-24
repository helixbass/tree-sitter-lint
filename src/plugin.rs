use std::sync::Arc;

use crate::{FromFileRunContextInstanceProviderFactory, Rule};

#[derive(Clone)]
pub struct Plugin<
    TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
> {
    pub name: String,
    pub rules: Vec<Arc<dyn Rule<TFromFileRunContextInstanceProviderFactory>>>,
}
