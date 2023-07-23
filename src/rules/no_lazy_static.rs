use std::sync::Arc;

use proc_macros::rule;
use tree_sitter::Node;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    context::QueryMatchContext,
    rule::{FileRunInfo, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta},
    violation, Config, ViolationBuilder,
};

pub struct NoLazyStaticRule {}

impl Rule for NoLazyStaticRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            name: "no_lazy_static".to_owned(),
            fixable: false,
            languages: vec![SupportedLanguage::Rust],
        }
    }

    fn instantiate(self: Arc<Self>, _config: &Config) -> Arc<dyn RuleInstance> {
        Arc::new(NoLazyStaticRuleInstance::new(self))
    }
}

struct NoLazyStaticRuleInstance {
    rule: Arc<NoLazyStaticRule>,
    listener_queries: Vec<RuleListenerQuery>,
}

impl NoLazyStaticRuleInstance {
    fn new(rule: Arc<NoLazyStaticRule>) -> Self {
        Self {
            rule,
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
}

impl RuleInstance for NoLazyStaticRuleInstance {
    fn instantiate_per_file(
        self: Arc<Self>,
        _file_run_info: &FileRunInfo,
    ) -> Arc<dyn RuleInstancePerFile> {
        Arc::new(NoLazyStaticRuleInstancePerFile::new(self))
    }

    fn rule(&self) -> Arc<dyn Rule> {
        self.rule.clone()
    }

    fn listener_queries(&self) -> &[RuleListenerQuery] {
        &self.listener_queries
    }
}

struct NoLazyStaticRuleInstancePerFile {
    rule_instance: Arc<NoLazyStaticRuleInstance>,
}

impl NoLazyStaticRuleInstancePerFile {
    fn new(rule_instance: Arc<NoLazyStaticRuleInstance>) -> Self {
        Self { rule_instance }
    }
}

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

    fn rule_instance(&self) -> Arc<dyn RuleInstance> {
        self.rule_instance.clone()
    }
}

pub fn no_lazy_static_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no_lazy_static",
        listeners => [
            r#"(
              (macro_invocation
                 macro: (identifier) @c (#eq? @c "lazy_static")
              )
            )"# => |node, context| {
                context.report(
                    violation! {
                        message => r#"Prefer 'OnceCell::*::Lazy' to 'lazy_static!()'"#,
                        node => node,
                    }
                );
            }
        ]
    }
}
