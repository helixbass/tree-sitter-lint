use std::rc::Rc;

use derive_builder::Builder;
use tree_sitter::{Node, Query};

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

        ResolvedRule::new(
            name,
            create(context)
                .into_iter()
                .map(|rule_listener| rule_listener.resolve(context))
                .collect(),
        )
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
    pub listeners: Vec<ResolvedRuleListener<'context>>,
}

impl<'context> ResolvedRule<'context> {
    pub fn new(name: String, listeners: Vec<ResolvedRuleListener<'context>>) -> Self {
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

impl<'on_query_match> RuleListener<'on_query_match> {
    pub fn resolve(self, context: &Context) -> ResolvedRuleListener<'on_query_match> {
        let RuleListener {
            query,
            capture_name,
            on_query_match,
        } = self;
        let query = Query::new(context.language, &query).unwrap();
        let capture_index = match capture_name {
            None => match query.capture_names().len() {
                0 => panic!("Expected capture"),
                _ => 0,
            },
            Some(capture_name) => query.capture_index_for_name(&capture_name).unwrap(),
        };
        ResolvedRuleListener {
            query,
            capture_index,
            on_query_match,
        }
    }
}

impl<'on_query_match> RuleListenerBuilder<'on_query_match> {
    pub fn on_query_match(&mut self, callback: impl Fn(&Node) + 'on_query_match) -> &mut Self {
        self.on_query_match = Some(Rc::new(callback));
        self
    }
}

pub struct ResolvedRuleListener<'on_query_match> {
    pub query: Query,
    pub capture_index: u32,
    pub on_query_match: Rc<dyn Fn(&Node) + 'on_query_match>,
}
