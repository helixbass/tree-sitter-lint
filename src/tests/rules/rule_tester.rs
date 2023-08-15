use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

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
