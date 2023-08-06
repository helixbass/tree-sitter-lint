use std::sync::Arc;

use crate::{FromFileRunContextInstanceProvider, Rule};

#[derive(Clone)]
pub struct Plugin<TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider> {
    pub name: String,
    pub rules: Vec<Arc<dyn Rule<TFromFileRunContextInstanceProvider>>>,
}
