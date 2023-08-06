use std::sync::Arc;

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use tree_sitter_grep::tree_sitter::Node;

use crate::{FromFileRunContextInstanceProvider, Rule, RuleTester};

#[test]
fn test_per_file_run_state() {
    RuleTester::run(
        no_more_than_5_uses_of_foo_rule(),
        rule_tests! {
            valid => [
                r#"
                    fn foo() {
                        let foo = foo;
                        foo();
                        foo();
                    }
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn foo() {
                            let foo = foo;
                            foo();
                            foo();
                            foo();
                        }
                    "#,
                    errors => [r#"Can't use 'foo' more than 5 times"#],
                },
            ]
        },
    );
}

fn no_more_than_5_uses_of_foo_rule<
    TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider,
>() -> Arc<dyn Rule<TFromFileRunContextInstanceProvider>> {
    rule! {
        name => "no_more_than_5_uses_of_foo",
        state => {
            [per-file-run]
            num_foos: usize
        },
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                self.num_foos += 1;
                if self.num_foos > 5 {
                    context.report(
                        violation! {
                            node => node,
                            message => r#"Can't use 'foo' more than 5 times"#,
                        }
                    );
                }
            }
        ],
        languages => [Rust],
    }
}

#[test]
fn test_store_node_in_per_file_run_state() {
    RuleTester::run(
        rule! {
            name => "stores-node-in-per-file-run-state",
            state => {
                [per-file-run]
                node: Option<Node<'a>>,
            },
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    self.node = Some(node);
                    context.report(violation! {
                        node => node,
                        message => "whee",
                    });
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
                    code => r#"fn whee() {}"#,
                    errors => [
                        {
                            message => "whee",
                        }
                    ],
                },
            ]
        },
    );
}
