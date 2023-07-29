#![cfg(test)]

use std::sync::Arc;

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

use crate::{rule::Rule, RuleTester};

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

fn no_more_than_5_uses_of_foo_rule() -> Arc<dyn Rule> {
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
            [per-run]
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
fn test_rule_messages_non_interpolated() {
    RuleTester::run(
        rule! {
            name => "has-non-interpolated-message",
            messages => [
                non_interpolated => "Not interpolated",
            ],
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message_id => "non_interpolated",
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [r#"Not interpolated"#],
                },
            ]
        },
    );
}

#[test]
fn test_rule_messages_interpolated() {
    RuleTester::run(
        rule! {
            name => "has-interpolated-message",
            messages => [
                interpolated => "Interpolated {{ foo }}",
            ],
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message_id => "interpolated",
                        data => {
                            foo => "bar"
                        }
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [r#"Interpolated bar"#],
                },
            ]
        },
    );
}

#[test]
fn test_rule_one_off_messages_interpolated() {
    RuleTester::run(
        rule! {
            name => "has-interpolated-message",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "Interpolated {{ foo }}",
                        data => {
                            foo => "bar"
                        }
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [r#"Interpolated bar"#],
                },
            ]
        },
    );
}

#[test]
fn test_rule_tests_message_id_and_data() {
    RuleTester::run(
        rule! {
            name => "has-interpolated-message",
            messages => [
                foo => "Interpolated {{ foo }}",
            ],
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message_id => "foo",
                        data => {
                            foo => "bar",
                        }
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [
                        {
                            message_id => "foo",
                            data => [
                                foo => "bar",
                            ]
                        }
                    ],
                },
            ]
        },
    );
}

#[test]
fn test_violation_type() {
    RuleTester::run(
        rule! {
            name => "reports-functions",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [
                        {
                            type => "function_item",
                        }
                    ],
                },
            ]
        },
    );
}

#[test]
fn test_data_key_named_type() {
    RuleTester::run(
        rule! {
            name => "has-interpolated-message",
            messages => [
                foo => "Interpolated {{ type }}",
            ],
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message_id => "foo",
                        data => {
                            type => "bar",
                        }
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
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => [
                        {
                            message_id => "foo",
                            data => [
                                type => "bar",
                            ]
                        }
                    ],
                },
            ]
        },
    );
}

#[test]
fn test_rule_options_optional() {
    RuleTester::run(
        rule! {
            name => "optional-options",
            options_type => Option<usize>,
            state => {
                [per-run]
                n: usize = options.unwrap_or(2),
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
        },
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
                },
                r#"
                    fn foo() {
                        foo();
                    }
                "#
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
                {
                    code => r#"
                        fn foo() {
                            let foo = foo;
                        }
                    "#,
                    errors => [r#"Can't use 'foo' more than 2 times"#],
                },
            ]
        },
    );
}
