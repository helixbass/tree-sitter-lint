use std::collections::HashMap;

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use serde::Deserialize;

use crate::{RuleTestValid, RuleTester};

#[test]
fn test_rule_test_errors_variable() {
    use crate::RuleTestExpectedErrorBuilder;

    let errors = [RuleTestExpectedErrorBuilder::default()
        .message("whee")
        .build()
        .unwrap()];
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
                    errors => errors,
                },
                {
                    code => r#"
                        fn bar() {}
                    "#,
                    errors => errors,
                },
            ]
        },
    );
}

#[test]
fn test_rule_test_spread_cases() {
    use crate::{RuleTestInvalid, RuleTestInvalidBuilder, RuleTestValidBuilder};

    fn valid_cases() -> Vec<RuleTestValid> {
        vec![RuleTestValidBuilder::default()
            .code("use bar::baz;")
            .build()
            .unwrap()]
    }

    let invalid_cases: Vec<RuleTestInvalid> = vec![RuleTestInvalidBuilder::default()
        .code("fn baz() {}")
        .errors(1)
        .build()
        .unwrap()];

    RuleTester::run(
        rule! {
            name => "reports-functions",
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
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
                ...valid_cases(),
                r#"
                    use foo::bar;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => 1,
                },
                ...invalid_cases,
                {
                    code => r#"
                        fn bar() {}
                    "#,
                    errors => 1,
                },
            ]
        },
    );
}

#[test]
fn test_rule_test_spread_cases_valid_just_str() {
    fn valid_cases() -> Vec<&'static str> {
        vec!["use bar::baz;"]
    }

    RuleTester::run(
        rule! {
            name => "reports-functions",
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
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
                ...valid_cases(),
                r#"
                    use foo::bar;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => 1,
                },
                {
                    code => r#"
                        fn bar() {}
                    "#,
                    errors => 1,
                },
            ]
        },
    );
}

#[test]
fn test_rule_test_null_option_value() {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Options {
        field: Option<String>,
    }

    RuleTester::run(
        rule! {
            name => "null-option-value",
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    if self.field.is_none() {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            options_type => Options,
            state => {
                [per-run]
                field: Option<String> = options.field.clone(),
            },
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => {
                        field => "abc",
                    }
                },
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => 1,
                },
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    errors => 1,
                    options => {
                        field => null,
                    }
                },
            ]
        },
    );
}

#[test]
fn test_rule_test_nested_arrow_separated_option_value() {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Options {
        field: HashMap<String, String>,
    }

    RuleTester::run(
        rule! {
            name => "nested-option-value",
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    if self.field.contains_key("foo") {
                        context.report(violation! {
                            node => node,
                            message => "whee",
                        });
                    }
                }
            ],
            options_type => Options,
            state => {
                [per-run]
                field: HashMap<String, String> = options.field.clone(),
            },
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => {
                        field => {
                            bar => "abc",
                        }
                    }
                },
                {
                    code => r#"
                        fn whee() {}
                    "#,
                },
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => {
                        field => { foo => "abc" },
                    },
                    errors => 1,
                },
            ]
        },
    );
}
