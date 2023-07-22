use std::sync::Arc;

use tree_sitter::Node;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    context::QueryMatchContext,
    rule::{FileRunInfo, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta},
    Config, ViolationBuilder,
};

pub struct NoDefaultDefaultRule {
    listener_queries: Vec<RuleListenerQuery>,
}

impl Rule for NoDefaultDefaultRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            name: "no_default_default".to_owned(),
            fixable: true,
            languages: vec![SupportedLanguage::Rust],
        }
    }

    fn listener_queries(&self) -> &[RuleListenerQuery] {
        &self.listener_queries
    }

    fn instantiate(&self, _config: &Config) -> Arc<dyn RuleInstance> {
        Arc::new(NoDefaultDefaultRuleInstance)
    }
}

struct NoDefaultDefaultRuleInstance;

impl RuleInstance for NoDefaultDefaultRuleInstance {
    fn instantiate_per_file(&self, _file_run_info: &FileRunInfo) -> Arc<dyn RuleInstancePerFile> {
        Arc::new(NoDefaultDefaultRuleInstancePerFile)
    }
}

struct NoDefaultDefaultRuleInstancePerFile;

impl RuleInstancePerFile for NoDefaultDefaultRuleInstancePerFile {
    fn on_query_match(&self, listener_index: usize, node: Node, context: &mut QueryMatchContext) {
        match listener_index {
            0 => {
                context.report(
                    ViolationBuilder::default()
                        .message(r#"Use '_d()' instead of 'Default::default()'"#)
                        .node(node)
                        .fix(|fixer| {
                            fixer.replace_text(node, "_d()");
                        })
                        .build()
                        .unwrap(),
                );
            }
            _ => unreachable!(),
        }
    }
}

pub fn no_default_default_rule() -> NoDefaultDefaultRule {
    NoDefaultDefaultRule {
        listener_queries: vec![RuleListenerQuery {
            query: r#"(
              (call_expression
                function:
                  (scoped_identifier
                    path:
                      (identifier) @first (#eq? @first "Default")
                    name:
                      (identifier) @second (#eq? @second "default")
                  )
              ) @c
            )"#
            .to_owned(),
            capture_name: Some("c".to_owned()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use proc_macros::rule_tests;

    use super::*;
    use crate::RuleTester;

    #[test]
    fn test_no_default_default_rule() {
        const ERROR_MESSAGE: &str = "Use '_d()' instead of 'Default::default()'";

        RuleTester::run(
            no_default_default_rule(),
            rule_tests! {
                valid => [
                    r#"
                        fn foo() {
                            let bar = Default::something_else::default();
                        }
                    "#,
                ],
                invalid => [
                    {
                        code => r#"
                            fn foo() {
                                let bar = Default::default();
                            }
                        "#,
                        errors => [ERROR_MESSAGE],
                        output => r#"
                            fn foo() {
                                let bar = _d();
                            }
                        "#,
                    },
                ]
            },
        );
    }
}
