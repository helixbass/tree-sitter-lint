use std::sync::Arc;

use derive_builder::Builder;
use tree_sitter::{Node, Query};

use crate::context::{Context, QueryMatchContext};

#[derive(Builder)]
#[builder(setter(into))]
pub struct Rule {
    pub name: String,
    #[allow(clippy::type_complexity)]
    #[builder(setter(custom))]
    pub create: Arc<dyn Fn(&Context) -> Vec<RuleListener>>,
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
        self.create = Some(Arc::new(callback));
        self
    }
}

#[macro_export]
macro_rules! rule {
    ($($variant:ident => $value:expr),* $(,)?) => {
        proc_macros::builder_args!(
            $crate::rule::RuleBuilder,
            $($variant => $value),*,
        )
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

type OnQueryMatchCallback<'a> = Arc<dyn Fn(Node, &QueryMatchContext) + 'a + Send + Sync>;

#[derive(Builder)]
#[builder(setter(into, strip_option))]
pub struct RuleListener<'on_query_match> {
    pub query: String,
    #[builder(default)]
    pub capture_name: Option<String>,
    #[builder(setter(custom))]
    pub on_query_match: OnQueryMatchCallback<'on_query_match>,
}

impl<'on_query_match> RuleListener<'on_query_match> {
    pub fn resolve(self, context: &Context) -> ResolvedRuleListener<'on_query_match> {
        let RuleListener {
            query: query_text,
            capture_name,
            on_query_match,
        } = self;
        let query = Query::new(context.language, &query_text).unwrap();
        let capture_index = match capture_name {
            None => match query.capture_names().len() {
                0 => panic!("Expected capture"),
                _ => 0,
            },
            Some(capture_name) => query.capture_index_for_name(&capture_name).unwrap(),
        };
        ResolvedRuleListener {
            query,
            query_text,
            capture_index,
            on_query_match,
        }
    }
}

impl<'on_query_match> RuleListenerBuilder<'on_query_match> {
    pub fn on_query_match(
        &mut self,
        callback: impl Fn(Node, &QueryMatchContext) + 'on_query_match + Send + Sync,
    ) -> &mut Self {
        self.on_query_match = Some(Arc::new(callback));
        self
    }
}

#[macro_export]
macro_rules! rule_listener {
    ($($variant:ident => $value:expr),* $(,)?) => {
        proc_macros::builder_args!(
            $crate::rule::RuleListenerBuilder,
            $($variant => $value),*,
        )
    }
}

pub struct ResolvedRuleListener<'on_query_match> {
    pub query: Query,
    pub query_text: String,
    pub capture_index: u32,
    pub on_query_match: OnQueryMatchCallback<'on_query_match>,
}
