use std::rc::Rc;

use derive_builder::Builder;
use tree_sitter::Node;

use crate::context::Context;

#[derive(Builder)]
#[builder(setter(into))]
pub struct Rule {
    pub name: String,
    #[builder(setter(custom))]
    pub create: Rc<dyn Fn(&Context) -> Vec<RuleListener>>,
}

impl Rule {
    pub fn resolve(self, context: &Context) -> ResolvedRule<'_> {
        let Rule { name, create } = self;

        ResolvedRule::new(name, create(context))
    }
}

impl RuleBuilder {
    pub fn create(
        &mut self,
        callback: impl Fn(&Context) -> Vec<RuleListener> + 'static,
    ) -> &mut Self {
        self.create = Some(Rc::new(callback));
        self
    }
}

pub struct ResolvedRule<'context> {
    pub name: String,
    pub listeners: Vec<RuleListener<'context>>,
}

impl<'context> ResolvedRule<'context> {
    pub fn new(name: String, listeners: Vec<RuleListener<'context>>) -> Self {
        Self { name, listeners }
    }
}

#[derive(Builder)]
#[builder(setter(into, strip_option))]
pub struct RuleListener<'on_query_match> {
    pub query: String,
    pub capture_name: Option<String>,
    #[builder(setter(custom))]
    pub on_query_match: Rc<dyn Fn(&Node) + 'on_query_match>,
}

impl<'on_query_match> RuleListenerBuilder<'on_query_match> {
    pub fn on_query_match(&mut self, callback: impl Fn(&Node) + 'on_query_match) -> &mut Self {
        self.on_query_match = Some(Rc::new(callback));
        self
    }
}
