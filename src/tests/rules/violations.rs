use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

use crate::{RuleTester, ViolationData};

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
fn test_rule_messages_multiple_interpolations() {
    RuleTester::run(
        rule! {
            name => "has-interpolated-message",
            messages => [
                interpolated => "{{ leading }} interpolated {{ middle }}{{ adjacent }} {{ single_space }} and {{ trailing }}",
            ],
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message_id => "interpolated",
                        data => {
                            leading => "foo",
                            middle => "bar",
                            adjacent => "baz",
                            single_space => "quux",
                            trailing => "whee",
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
                    errors => [r#"foo interpolated barbaz quux and whee"#],
                },
            ]
        },
    );
}

#[test]
fn test_violation_range() {
    RuleTester::run(
        rule! {
            name => "reports-range",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "whee",
                        range => node.child_by_field_name("name").unwrap().range(),
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
                            column => 4,
                        }
                    ],
                },
            ]
        },
    );
}

#[test]
fn test_self_field_in_data() {
    RuleTester::run(
        rule! {
            name => "uses-self-in-data",
            state => {
                [per-run]
                foo: &'static str = "abc",
            },
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "{{whee}}",
                        data => {
                            whee => self.foo,
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
                    code => r#"fn whee() {}"#,
                    errors => [
                        {
                            message => "abc",
                        }
                    ],
                },
            ]
        },
    );
}

#[test]
fn test_data_variable() {
    RuleTester::run(
        rule! {
            name => "uses-data-as-variable",
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    let data: ViolationData = [
                        (
                            "whee".to_owned(),
                            "bar".to_owned(),
                        )
                    ].into();
                    context.report(violation! {
                        node => node,
                        message => "{{whee}}",
                        data => data.clone(),
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
                            message => "bar",
                        }
                    ],
                },
            ]
        },
    );
}
