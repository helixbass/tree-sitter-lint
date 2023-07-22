#![cfg(test)]

use std::sync::Arc;

use proc_macros::rule;
use tree_sitter::Node;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    context::QueryMatchContext,
    rule::{FileRunInfo, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta},
    run_fixing_for_slice, Config, ConfigBuilder, ViolationBuilder,
};

#[test]
fn test_single_fix() {
    let mut file_contents = r#"
        fn foo() {}
    "#
    .to_owned()
    .into_bytes();
    run_fixing_for_slice(
        &mut file_contents,
        "tmp.rs",
        ConfigBuilder::default()
            .rules([create_identifier_replacing_rule("foo", "bar")])
            .fix(true)
            .build()
            .unwrap(),
    );
    assert_eq!(
        std::str::from_utf8(&file_contents).unwrap().trim(),
        r#"
            fn bar() {}
        "#
        .trim()
    );
}

struct IdentifierReplacingRule {
    name: String,
    replacement: String,
    listener_queries: Vec<RuleListenerQuery>,
}

impl IdentifierReplacingRule {
    pub fn new(name: String, replacement: String) -> Self {
        Self {
            listener_queries: vec![RuleListenerQuery {
                query: format!(
                    r#"(
                      (identifier) @c (#eq? @c "{}")
                    )"#,
                    name
                ),
                capture_name: None,
            }],
            name,
            replacement,
        }
    }
}

impl Rule for IdentifierReplacingRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            name: format!("replace_{}_with_{}", self.name, self.replacement),
            fixable: true,
            languages: vec![SupportedLanguage::Rust],
        }
    }

    fn listener_queries(&self) -> &[RuleListenerQuery] {
        &self.listener_queries
    }

    fn instantiate(self: Arc<Self>, _config: &Config) -> Arc<dyn RuleInstance> {
        Arc::new(IdentifierReplacingRuleInstance::new(self))
    }
}

struct IdentifierReplacingRuleInstance {
    rule: Arc<IdentifierReplacingRule>,
}

impl IdentifierReplacingRuleInstance {
    fn new(rule: Arc<IdentifierReplacingRule>) -> Self {
        Self { rule }
    }
}

impl RuleInstance for IdentifierReplacingRuleInstance {
    fn instantiate_per_file(
        self: Arc<Self>,
        _file_run_info: &FileRunInfo,
    ) -> Arc<dyn RuleInstancePerFile> {
        Arc::new(IdentifierReplacingRuleInstancePerFile::new(self))
    }

    fn rule(&self) -> Arc<dyn Rule> {
        self.rule.clone()
    }
}

struct IdentifierReplacingRuleInstancePerFile {
    rule_instance: Arc<IdentifierReplacingRuleInstance>,
}

impl IdentifierReplacingRuleInstancePerFile {
    fn new(rule_instance: Arc<IdentifierReplacingRuleInstance>) -> Self {
        Self { rule_instance }
    }
}

impl RuleInstancePerFile for IdentifierReplacingRuleInstancePerFile {
    fn on_query_match(&self, listener_index: usize, node: Node, context: &mut QueryMatchContext) {
        match listener_index {
            0 => {
                context.report(
                    ViolationBuilder::default()
                        .message(format!(
                            r#"Use '{}' instead of '{}'"#,
                            self.rule_instance.rule.replacement, self.rule_instance.rule.name,
                        ))
                        .node(node)
                        .fix(|fixer| {
                            fixer.replace_text(node, &self.rule_instance.rule.replacement);
                        })
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

fn create_identifier_replacing_rule(
    name: impl Into<String>,
    replacement: impl Into<String>,
) -> Arc<dyn Rule> {
    let name = name.into();
    let replacement = replacement.into();
    Arc::new(IdentifierReplacingRule::new(name, replacement))
}

// fn create_identifier_replacing_rulez(
//     name: impl Into<String>,
//     replacement: impl Into<String>,
// ) -> Arc<dyn Rule> { rule! { name => rule_name, fixable => true, state => {
//   [rule-static] name: String = name.into(), replacement: String =
//   replacement.into(), }, listeners => [ format!(r#"( (identifier) @c (#eq? @c
//   "{}") )"#, self.name) => |node, context| { context.report(
//   ViolationBuilder::default() .message( format!(r#"Use '{}' instead of
//   '{}'"#, self.replacement, self.name) ) .node(node) .fix(|fixer| {
//   fixer.replace_text(node, &self.replacement); }) .build() .unwrap(), ); } ]
//   }
// }
