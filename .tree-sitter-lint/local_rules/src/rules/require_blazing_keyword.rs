use std::sync::Arc;

use tree_sitter_lint::{rule, violation, FromFileRunContextInstanceProviderFactory, Rule};

pub fn require_blazing_keyword_rule<T: FromFileRunContextInstanceProviderFactory>(
) -> Arc<dyn Rule<T>> {
    rule! {
        name => "require-blazing-keyword",
        languages => [Toml],
        listeners => [
            r#"(
              (pair
                (bare_key) @key (#eq? @key "keywords")
                (array) @value
              )
            )"# => {
                capture_name => "value",
                callback => |node, context| {
                    for child_index in 0..node.child_count() {
                        let child = node.child(child_index).unwrap();
                        if context.get_node_text(child).contains("blazing") {
                            return;
                        }
                    }

                    context.report(violation! {
                        node => node,
                        message => "Expected to find keyword containing 'blazing'",
                    })
                }
            }
        ]
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter_lint::{rule_tests, RuleTester};

    use super::*;

    #[test]
    fn test_require_blazing_keyword_rule() {
        const ERROR_MESSAGE: &str = "Expected to find keyword containing 'blazing'";

        RuleTester::run(
            require_blazing_keyword_rule(),
            rule_tests! {
                valid => [
                    r#"
                        [package]
                        keywords = ["something", "not blazing"]
                    "#,
                    // no keywords
                    r#"
                        [package]
                        categories = ["foo"]
                    "#,
                ],
                invalid => [
                    {
                        code => r#"
                            [package]
                            keywords = ["something", "else"]
                        "#,
                        errors => [ERROR_MESSAGE],
                    },
                    {
                        code => r#"
                            [package]
                            keywords = []
                        "#,
                        errors => [ERROR_MESSAGE],
                    },
                ]
            },
        );
    }
}
