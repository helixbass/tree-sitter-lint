use std::sync::Arc;

use tree_sitter::Node;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    context::QueryMatchContext,
    rule::{FileRunInfo, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta},
    Config, ViolationBuilder,
};

pub struct NoLazyStaticRule {
    listener_queries: Vec<RuleListenerQuery>,
}

impl Rule for NoLazyStaticRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            name: "no_lazy_static".to_owned(),
            fixable: false,
            languages: vec![SupportedLanguage::Rust],
        }
    }

    fn listener_queries(&self) -> &[RuleListenerQuery] {
        &self.listener_queries
    }

    fn instantiate(&self, _config: &Config) -> Arc<dyn RuleInstance> {
        Arc::new(NoLazyStaticRuleInstance)
    }
}

struct NoLazyStaticRuleInstance;

impl RuleInstance for NoLazyStaticRuleInstance {
    fn instantiate_per_file(&self, _file_run_info: &FileRunInfo) -> Arc<dyn RuleInstancePerFile> {
        Arc::new(NoLazyStaticRuleInstancePerFile)
    }
}

struct NoLazyStaticRuleInstancePerFile;

impl RuleInstancePerFile for NoLazyStaticRuleInstancePerFile {
    fn on_query_match(&self, listener_index: usize, node: Node, context: &mut QueryMatchContext) {
        match listener_index {
            0 => {
                context.report(
                    ViolationBuilder::default()
                        .message(r#"Prefer 'OnceCell::*::Lazy' to 'lazy_static!()'"#)
                        .node(node)
                        .build()
                        .unwrap(),
                );
            }
            _ => unreachable!(),
        }
    }
}

pub fn no_lazy_static_rule() -> NoLazyStaticRule {
    NoLazyStaticRule {
        listener_queries: vec![RuleListenerQuery {
            query: r#"(
              (macro_invocation
                 macro: (identifier) @c (#eq? @c "lazy_static")
              )
            )"#
            .to_owned(),
            capture_name: None,
        }],
    }
}
