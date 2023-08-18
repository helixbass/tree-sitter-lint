use std::sync::Arc;

use crate::Rule;

#[derive(Clone)]
pub struct Plugin {
    pub name: String,
    pub rules: Vec<Arc<dyn Rule>>,
}
