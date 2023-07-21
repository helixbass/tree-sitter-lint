use std::sync::Arc;

use derive_builder::Builder;
use tree_sitter::{Node, Query};

use crate::{context::QueryMatchContext, Args};

#[derive(Clone)]
pub struct RuleMeta {
    pub name: String,
    pub fixable: bool,
}

pub struct Rule {
    pub meta: RuleMeta,
    #[allow(clippy::type_complexity)]
    pub create: Arc<dyn Fn(&Args) -> Vec<RuleListener>>,
}

impl Rule {
    pub fn resolve(self, args: &Args) -> ResolvedRule<'_> {
        let Rule { meta, create } = self;

        ResolvedRule::new(
            meta,
            create(args)
                .into_iter()
                .map(|rule_listener| rule_listener.resolve(args))
                .collect(),
        )
    }
}

#[derive(Clone, Default)]
pub struct RuleBuilder {
    name: Option<String>,
    fixable: bool,
    create: Option<Arc<dyn Fn(&Args) -> Vec<RuleListener>>>,
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

    pub fn create(&mut self, callback: impl Fn(&Args) -> Vec<RuleListener> + 'static) -> &mut Self {
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

pub struct ResolvedRule<'a> {
    pub meta: RuleMeta,
    pub listeners: Vec<ResolvedRuleListener<'a>>,
}

impl<'a> ResolvedRule<'a> {
    pub fn new(meta: RuleMeta, listeners: Vec<ResolvedRuleListener<'a>>) -> Self {
        Self { meta, listeners }
    }
}

type OnQueryMatchCallback<'a> = Arc<dyn Fn(Node, &mut QueryMatchContext) + 'a + Send + Sync>;

#[derive(Builder)]
#[builder(setter(into, strip_option))]
pub struct RuleListener<'a> {
    pub query: String,
    #[builder(default)]
    pub capture_name: Option<String>,
    #[builder(setter(custom))]
    pub on_query_match: OnQueryMatchCallback<'a>,
}

impl<'a> RuleListener<'a> {
    pub fn resolve(self, args: &Args) -> ResolvedRuleListener<'a> {
        let RuleListener {
            query: query_text,
            capture_name,
            on_query_match,
        } = self;
        let query = Query::new(args.language.language(), &query_text).unwrap();
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

impl<'a> RuleListenerBuilder<'a> {
    pub fn on_query_match(
        &mut self,
        callback: impl Fn(Node, &mut QueryMatchContext) + 'a + Send + Sync,
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

pub struct ResolvedRuleListener<'a> {
    pub query: Query,
    pub query_text: String,
    pub capture_index: u32,
    pub on_query_match: OnQueryMatchCallback<'a>,
}
