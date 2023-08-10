use std::sync::Arc;

use crate::{EventEmitterFactory, Rule};

#[derive(Clone)]
pub struct Plugin {
    pub name: String,
    pub rules: Vec<Arc<dyn Rule>>,
    pub event_emitter_factories: Vec<Arc<dyn EventEmitterFactory>>,
}
