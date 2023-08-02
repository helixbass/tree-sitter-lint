use std::{collections::HashMap, ops, sync::Arc};

use tree_sitter_grep::{tree_sitter::QueryMatch, SupportedLanguage};

use crate::{
    config::{PluginIndex, RuleConfiguration},
    context::QueryMatchContext,
    tree_sitter::{Language, Node, Query},
    Config,
};

#[derive(Clone, Debug)]
pub struct RuleMeta {
    pub name: String,
    pub fixable: bool,
    pub languages: Vec<SupportedLanguage>,
    pub messages: Option<HashMap<String, String>>,
}

pub trait Rule: Send + Sync {
    fn meta(&self) -> RuleMeta;
    fn instantiate(
        self: Arc<Self>,
        config: &Config,
        rule_configuration: &RuleConfiguration,
    ) -> Arc<dyn RuleInstance>;
}

pub trait RuleInstance: Send + Sync {
    fn instantiate_per_file(
        self: Arc<Self>,
        file_run_info: &FileRunInfo,
    ) -> Box<dyn RuleInstancePerFile>;
    fn rule(&self) -> Arc<dyn Rule>;
    fn listener_queries(&self) -> &[RuleListenerQuery];
}

pub struct InstantiatedRule {
    pub meta: RuleMeta,
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

pub enum NodeOrCaptures<'a> {
    Node(Node<'a>),
    Captures(Captures<'a>),
}

impl<'a> From<Node<'a>> for NodeOrCaptures<'a> {
    fn from(value: Node<'a>) -> Self {
        Self::Node(value)
    }
}

impl<'a> From<Captures<'a>> for NodeOrCaptures<'a> {
    fn from(value: Captures<'a>) -> Self {
        Self::Captures(value)
    }
}

#[derive(Debug)]
pub struct Captures<'a> {
    pub query_match: &'a QueryMatch<'a, 'a>,
    pub query: Arc<Query>,
}

impl<'a> Captures<'a> {
    pub fn new(query_match: &'a QueryMatch<'a, 'a>, query: Arc<Query>) -> Self {
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

    pub fn get_all(&self, capture_name: &str) -> impl Iterator<Item = Node> {
        self.query_match
            .nodes_for_capture_index(self.query.capture_index_for_name(capture_name).unwrap())
    }
}

impl<'a> ops::Index<&str> for Captures<'a> {
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

pub trait RuleInstancePerFile {
    fn on_query_match(
        &mut self,
        listener_index: usize,
        node_or_captures: NodeOrCaptures,
        context: &mut QueryMatchContext,
    );
    fn rule_instance(&self) -> Arc<dyn RuleInstance>;
}

pub struct FileRunInfo {}

pub enum MatchBy {
    PerCapture { capture_name: Option<String> },
    PerMatch,
}

pub struct RuleListenerQuery {
    pub query: String,
    pub match_by: MatchBy,
}

impl RuleListenerQuery {
    pub fn resolve(&self, language: Language) -> ResolvedRuleListenerQuery {
        let query = Query::new(language, &self.query).unwrap();
        let resolved_match_by = match &self.match_by {
            MatchBy::PerCapture { capture_name } => ResolvedMatchBy::PerCapture {
                capture_index: match capture_name.as_ref() {
                    None => match query.capture_names().len() {
                        0 => panic!("Expected capture"),
                        _ => 0,
                    },
                    Some(capture_name) => query.capture_index_for_name(capture_name).unwrap(),
                },
            },
            MatchBy::PerMatch => ResolvedMatchBy::PerMatch,
        };
        ResolvedRuleListenerQuery {
            query,
            query_text: self.query.clone(),
            match_by: resolved_match_by,
        }
    }
}

pub enum ResolvedMatchBy {
    PerCapture { capture_index: u32 },
    PerMatch,
}

pub struct ResolvedRuleListenerQuery {
    pub query: Query,
    pub query_text: String,
    pub match_by: ResolvedMatchBy,
}

impl ResolvedRuleListenerQuery {
    pub fn capture_name(&self) -> &str {
        match &self.match_by {
            ResolvedMatchBy::PerCapture { capture_index } => {
                &self.query.capture_names()[*capture_index as usize]
            }
            _ => panic!("Called capture_name() for PerMatch"),
        }
    }
}

pub type RuleOptions = serde_json::Value;
