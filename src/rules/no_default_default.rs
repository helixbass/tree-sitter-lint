use std::sync::Arc;

use proc_macros::rule;

use crate::{rule::Rule, violation};

pub fn no_default_default_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no_default_default",
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
        ]
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
