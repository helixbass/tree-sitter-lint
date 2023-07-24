use std::sync::Arc;

use tree_sitter::{Language, Node, Query};
use tree_sitter_grep::SupportedLanguage;

use crate::{config::PluginIndex, context::QueryMatchContext, Config};

#[derive(Clone)]
pub struct RuleMeta {
    pub name: String,
    pub fixable: bool,
    pub languages: Vec<SupportedLanguage>,
}

pub trait Rule: Send + Sync {
    fn meta(&self) -> RuleMeta;
    fn instantiate(self: Arc<Self>, config: &Config) -> Arc<dyn RuleInstance>;
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
    plugin_index: Option<PluginIndex>,
}

impl InstantiatedRule {
    pub fn new(rule: Arc<dyn Rule>, plugin_index: Option<PluginIndex>, config: &Config) -> Self {
        Self {
            meta: rule.meta(),
            rule_instance: rule.clone().instantiate(config),
            rule,
            plugin_index,
        }
    }
}

pub trait RuleInstancePerFile: Send + Sync {
    fn on_query_match(
        &mut self,
        listener_index: usize,
        node: Node,
        context: &mut QueryMatchContext,
    );
    fn rule_instance(&self) -> Arc<dyn RuleInstance>;
}

pub struct FileRunInfo {}

pub struct RuleListenerQuery {
    pub query: String,
    pub capture_name: Option<String>,
}

impl RuleListenerQuery {
    pub fn resolve(&self, language: Language) -> ResolvedRuleListenerQuery {
        let query = Query::new(language, &self.query).unwrap();
        let capture_index = match self.capture_name.as_ref() {
            None => match query.capture_names().len() {
                0 => panic!("Expected capture"),
                _ => 0,
            },
            Some(capture_name) => query.capture_index_for_name(capture_name).unwrap(),
        };
        ResolvedRuleListenerQuery {
            query,
            query_text: self.query.clone(),
            capture_index,
        }
    }
}

pub struct ResolvedRuleListenerQuery {
    pub query: Query,
    pub query_text: String,
    pub capture_index: u32,
}

impl ResolvedRuleListenerQuery {
    pub fn capture_name(&self) -> &str {
        &self.query.capture_names()[self.capture_index as usize]
    }
}
