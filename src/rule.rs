use std::sync::Arc;

use derive_builder::Builder;
use tree_sitter::{Node, Query};

use crate::context::{Context, QueryMatchContext};

#[derive(Clone)]
pub struct RuleMeta {
    pub name: String,
    pub fixable: bool,
}

pub struct Rule {
    pub meta: RuleMeta,
    #[allow(clippy::type_complexity)]
    pub create: Arc<dyn Fn(&Context) -> Vec<RuleListener>>,
}

impl Rule {
    pub fn resolve(self, context: &Context) -> ResolvedRule<'_> {
        let Rule { meta, create } = self;

        ResolvedRule::new(
            meta,
            create(context)
                .into_iter()
                .map(|rule_listener| rule_listener.resolve(context))
                .collect(),
        )
    }
}

#[derive(Clone, Default)]
pub struct RuleBuilder {
    name: Option<String>,
    fixable: bool,
    create: Option<Arc<dyn Fn(&Context) -> Vec<RuleListener>>>,
}

impl RuleBuilder {
    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = Some(name.into());
        self
    }

    pub fn fixable(&mut self, fixable: bool) -> &mut Self {
        self.fixable = fixable;
        self
    }

    pub fn create(
        &mut self,
        callback: impl Fn(&Context) -> Vec<RuleListener> + 'static,
    ) -> &mut Self {
        self.create = Some(Arc::new(callback));
        self
    }

    pub fn build(&self) -> Result<Rule, ()> {
        Ok(Rule {
            meta: RuleMeta {
                name: self.name.clone().ok_or(())?,
                fixable: self.fixable,
            },
            create: self.create.clone().ok_or(())?,
        })
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
    pub meta: RuleMeta,
    pub listeners: Vec<ResolvedRuleListener<'context>>,
}

impl<'context> ResolvedRule<'context> {
    pub fn new(meta: RuleMeta, listeners: Vec<ResolvedRuleListener<'context>>) -> Self {
        Self { meta, listeners }
    }
}

type OnQueryMatchCallback<'a> = Arc<dyn Fn(Node, &mut QueryMatchContext) + 'a + Send + Sync>;

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
        callback: impl Fn(Node, &mut QueryMatchContext) + 'on_query_match + Send + Sync,
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
