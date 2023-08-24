use std::{collections::HashMap, ops, sync::Arc};

use tracing::{instrument, trace_span};
use tree_sitter_grep::{tree_sitter::QueryMatch, SupportedLanguage};

use crate::{
    config::{PluginIndex, RuleConfiguration},
    context::{FileRunContext, QueryMatchContext},
    tree_sitter::{Language, Node, Query},
    Config,
};

#[derive(Clone, Debug)]
pub struct RuleMeta {
    pub name: String,
    pub fixable: bool,
    pub languages: Vec<SupportedLanguage>,
    pub messages: Option<HashMap<String, String>>,
    pub allow_self_conflicting_fixes: bool,
}

pub trait Rule: Send + Sync {
    fn meta(&self) -> Arc<RuleMeta>;
    fn instantiate(
        self: Arc<Self>,
        config: &Config,
        rule_configuration: &RuleConfiguration,
    ) -> Arc<dyn RuleInstance>;
}

pub trait RuleInstance: Send + Sync {
    fn instantiate_per_file<'a>(
        self: Arc<Self>,
        file_run_context: FileRunContext<'a, '_>,
    ) -> Box<dyn RuleInstancePerFile<'a> + 'a>;
    fn rule(&self) -> Arc<dyn Rule>;
    fn listener_queries(&self) -> &[RuleListenerQuery];
}

pub struct InstantiatedRule {
    pub meta: Arc<RuleMeta>,
    pub rule: Arc<dyn Rule>,
    pub rule_instance: Arc<dyn RuleInstance>,
    pub plugin_index: Option<PluginIndex>,
}

impl InstantiatedRule {
    pub fn new(
        rule: Arc<dyn Rule>,
        plugin_index: Option<PluginIndex>,
        rule_configuration: &RuleConfiguration,
        config: &Config,
    ) -> Self {
        Self {
            meta: rule.meta(),
            rule_instance: rule.clone().instantiate(config, rule_configuration),
            rule,
            plugin_index,
        }
    }
}

pub enum NodeOrCaptures<'a, 'b> {
    Node(Node<'a>),
    Captures(Captures<'a, 'b>),
}

impl<'a, 'b> From<Node<'a>> for NodeOrCaptures<'a, 'b> {
    fn from(value: Node<'a>) -> Self {
        Self::Node(value)
    }
}

impl<'a, 'b> From<Captures<'a, 'b>> for NodeOrCaptures<'a, 'b> {
    fn from(value: Captures<'a, 'b>) -> Self {
        Self::Captures(value)
    }
}

#[derive(Debug)]
pub struct Captures<'a, 'b> {
    pub query_match: &'b QueryMatch<'a, 'a>,
    pub query: Arc<Query>,
}

impl<'a, 'b> Captures<'a, 'b> {
    pub fn new(query_match: &'b QueryMatch<'a, 'a>, query: Arc<Query>) -> Self {
        Self { query_match, query }
    }

    pub fn get(&self, capture_name: &str) -> Option<Node> {
        let mut nodes_for_capture_index = self
            .query_match
            .nodes_for_capture_index(self.query.capture_index_for_name(capture_name).unwrap());
        let first_node = nodes_for_capture_index.next()?;
        if nodes_for_capture_index.next().is_some() {
            panic!("Use .all() for captures that correspond to multiple nodes");
        }
        Some(first_node)
    }

    pub fn get_all(&self, capture_name: &str) -> impl Iterator<Item = Node<'a>> + 'b {
        self.query_match
            .nodes_for_capture_index(self.query.capture_index_for_name(capture_name).unwrap())
    }
}

impl<'a, 'b> ops::Index<&str> for Captures<'a, 'b> {
    type Output = Node<'a>;

    fn index(&self, capture_name: &str) -> &Self::Output {
        let capture_index = self.query.capture_index_for_name(capture_name).unwrap();
        let mut captures_for_this_capture_index = self
            .query_match
            .captures
            .iter()
            .filter(|capture| capture.index == capture_index);
        let first_capture = captures_for_this_capture_index
            .next()
            .unwrap_or_else(|| panic!("Capture '{capture_name}' had no nodes"));
        if captures_for_this_capture_index.next().is_some() {
            panic!("Use .all() for captures that correspond to multiple nodes");
        }
        &first_capture.node
    }
}

pub trait RuleInstancePerFile<'a> {
    fn on_query_match<'b>(
        &mut self,
        listener_index: usize,
        node_or_captures: NodeOrCaptures<'a, 'b>,
        context: &QueryMatchContext<'a, '_>,
    );
    fn rule_instance(&self) -> Arc<dyn RuleInstance>;
}

#[derive(Debug)]
pub enum MatchBy {
    PerCapture { capture_name: Option<String> },
    PerMatch,
}

#[derive(Debug)]
pub struct RuleListenerQuery {
    pub query: String,
    pub match_by: MatchBy,
}

impl RuleListenerQuery {
    #[instrument(level = "trace")]
    pub fn resolve(&self, language: Language) -> ResolvedRuleListenerQuery {
        let span = trace_span!("parse individual rule listener query").entered();

        let query = Query::new(language, &self.query).unwrap();

        span.exit();

        let span = trace_span!("resolve capture name").entered();

        let resolved_match_by = match &self.match_by {
            MatchBy::PerCapture { capture_name } => ResolvedMatchBy::PerCapture {
                capture_name: match capture_name.as_ref() {
                    None => match query.capture_names().len() {
                        0 => panic!("Expected capture"),
                        _ => query.capture_names()[0].clone(),
                    },
                    // Some(capture_name) => query.capture_index_for_name(capture_name).unwrap(),
                    Some(capture_name) => capture_name.clone(),
                },
            },
            MatchBy::PerMatch => ResolvedMatchBy::PerMatch,
        };

        span.exit();

        ResolvedRuleListenerQuery {
            query,
            query_text: self.query.clone(),
            match_by: resolved_match_by,
        }
    }
}

pub enum ResolvedMatchBy {
    PerCapture { capture_name: String },
    PerMatch,
}

pub struct ResolvedRuleListenerQuery {
    pub query: Query,
    pub query_text: String,
    pub match_by: ResolvedMatchBy,
}

pub type RuleOptions = serde_json::Value;
