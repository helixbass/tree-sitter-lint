use std::sync::Arc;

use tree_sitter_lint::{rule, violation, Rule};

pub fn no_default_default_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no-default-default",
        fixable => true,
        listeners => [
            r#"(
              (call_expression
                function:
                  (scoped_identifier
                    path:
                      (identifier) @first (#eq? @first "Default")
                    name:
                      (identifier) @second (#eq? @second "default")
                  )
              ) @c
            )"# => {
                capture_name => "c",
                callback => |node, context| {
                    context.report(
                        violation! {
                            message => r#"Use '_d()' instead of 'Default::default()'"#,
                            node => node,
                            fix => |fixer| {
                                fixer.replace_text(node, "_d()");
                            },
                        }
                    );
                }
            }
        ],
        languages => [Rust],
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter_lint::{rule_tests, RuleTester};

    use super::*;

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
