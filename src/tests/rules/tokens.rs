use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use squalid::OptionExt;
use tree_sitter_grep::tree_sitter::Node;

use crate::RuleTester;

#[test]
fn test_get_token_after() {
    RuleTester::run(
        rule! {
            name => "uses-get-token-after",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    if context.maybe_get_token_after(node, Option::<fn(Node) -> bool>::None)
                        .matches(|next_token| context.get_node_text(next_token) == "mod") {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    use foo::bar;
                "#,
                r#"
                    fn foo() {}
                    fn bar() {}
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}

                        mod foo;
                    "#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_get_last_token() {
    RuleTester::run(
        rule! {
            name => "uses-get-last-token",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    if context.get_node_text(
                        context.get_last_token(node, Option::<fn(Node) -> bool>::None)
                    ) == "}" {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    use foo::bar;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_comments_exist_between() {
    RuleTester::run(
        rule! {
            name => "uses-comments-exist-between",
            listeners => [
                r#"(
                  (if_expression
                    condition: (_) @condition
                    alternative: (else_clause
                      (block) @else
                    )
                  )
                )"# => |captures, context| {
                    if context.comments_exist_between(
                        captures["condition"], captures["else"]
                    ) {
                        context.report(violation! {
                            node => captures["condition"],
                            message => "whee",
                        });
                    }
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    if foo {
                        bar();
                    } else {
                        // inside else block
                        baz();
                    }
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        if foo {
                            bar();
                            // inside if block
                        } else {
                            baz();
                        }
                    "#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_get_tokens_between() {
    RuleTester::run(
        rule! {
            name => "uses-get-tokens-between",
            listeners => [
                r#"(
                  (use_declaration) @c
                )"# => |node, context| {
                    if context.get_tokens_between(
                        context.get_first_token(node, Option::<fn(Node) -> bool>::None),
                        context.get_last_token(node, Option::<fn(Node) -> bool>::None),
                        Option::<fn(Node) -> bool>::None
                    ).count() == 3 {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    use foo::bar::baz;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        use foo::bar;
                    "#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_get_comments_after() {
    RuleTester::run(
        rule! {
            name => "uses-get-comments-after",
            listeners => [
                r#"(
                  (use_declaration) @c
                )"# => |node, context| {
                    if context.get_comments_after(
                        node
                    ).count() == 2 {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    use foo::bar::baz;
                "#,
                r#"
                    use foo::bar::baz;
                    // one comment
                "#,
                r#"
                    use foo::bar::baz;
                    // one comment
                    /* two comments */
                    // three comments
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        use foo::bar;
                        // one comment
                        /* two comments */
                    "#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}
