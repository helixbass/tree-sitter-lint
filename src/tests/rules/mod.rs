#![cfg(test)]

use std::sync::Arc;

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

mod meta;
mod methods;
mod options;
mod provided_types;
mod rule_tester;
mod state;
mod tokens;
mod violations;

use crate::{rule::Rule, RuleTester};

#[test]
fn test_rule_options() {
    RuleTester::run(
        no_more_than_n_uses_of_foo_rule(),
        rule_tests! {
            valid => [
                {
                    code => r#"
                        fn foo() {
                            let foo = foo;
                            foo();
                            foo();
                        }
                    "#,
                    options => 5,
                }
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
                    options => 5,
                    errors => [r#"Can't use 'foo' more than 5 times"#],
                },
            ]
        },
    );
}

fn no_more_than_n_uses_of_foo_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no_more_than_n_uses_of_foo",
        options_type => usize,
        state => {
            [per-config]
            n: usize = options,
            [per-file-run]
            num_foos: usize
        },
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                self.num_foos += 1;
                if self.num_foos > self.n {
                    context.report(
                        violation! {
                            node => node,
                            message => format!(r#"Can't use 'foo' more than {} times"#, self.n),
                        }
                    );
                }
            }
        ],
        languages => [Rust],
    }
}

#[test]
fn test_rule_per_match_callback() {
    RuleTester::run(
        rule! {
            name => "per-match-callback",
            listeners => [
                r#"(
                  (use_declaration
                    argument: (scoped_identifier
                      path: (identifier) @first
                      name: (identifier) @second
                    )
                  )
                )"# => |captures, context| {
                    let first = captures["first"];
                    if context.get_node_text(first) != "foo" {
                        context.report(violation! {
                            node => first,
                            message => "Not foo",
                        });
                    }

                    let second = captures["second"];
                    if context.get_node_text(second) != "bar" {
                        context.report(violation! {
                            node => second,
                            message => "Not bar",
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
                    fn whee() {}
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        use foo::something_else;
                    "#,
                    errors => [r#"Not bar"#],
                },
                {
                    code => r#"
                        use something_else::bar;
                    "#,
                    errors => [r#"Not foo"#],
                },
            ]
        },
    );
}

#[test]
fn test_root_exit_listener() {
    RuleTester::run(
        rule! {
            name => "uses-root-exit-listener",
            listeners => [
                "source_file:exit" => |node, context| {
                    let mut cursor = node.walk();
                    if node.named_children(&mut cursor).count() != 1 {
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
                    code => r#""#,
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

#[test]
fn test_root_exit_listener_amid_other_listeners() {
    RuleTester::run(
        rule! {
            name => "uses-root-exit-listener",
            listeners => [
                r#"(function_item) @c"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "function",
                    });
                },
                "source_file:exit" => |node, context| {
                    let mut cursor = node.walk();
                    if node.named_children(&mut cursor).count() != 1 {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                },
                r#"(use_declaration) @c"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "use declaration",
                    });
                },
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    mod foo;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        use foo::bar;
                        fn bar() {}
                    "#,
                    errors => [
                        {
                            message => "whee",
                        },
                        {
                            message => "use declaration",
                        },
                        {
                            message => "function",
                        }
                    ],
                },
            ]
        },
    );
}
