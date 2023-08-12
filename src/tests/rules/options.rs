use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use serde::Deserialize;
use serde_json::json;

use crate::RuleTester;

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

#[test]
fn test_options_struct() {
    #[derive(Deserialize)]
    struct Options {
        whee: String,
    }

    RuleTester::run(
        rule! {
            name => "has-options-struct",
            options_type! => Options,
            state => {
                [per-run]
                whee: String = options.whee,
            },
            languages => [Rust],
            listeners => [
                "(function_item) @c" => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => self.whee.clone(),
                    });
                }
            ]
        },
        rule_tests! {
            valid => [
                {
                    code => r#"
                        use foo::bar;
                    "#,
                    options => { whee => "abc" },
                }
            ],
            invalid => [
                {
                    code => r#"
                        fn foo() {}
                    "#,
                    options => { whee => "def" },
                    errors => ["def"],
                },
            ]
        },
    );
}

#[test]
fn test_options_list() {
    #[derive(Deserialize)]
    struct OptionType {
        #[allow(dead_code)]
        foo: String,
    }

    RuleTester::run(
        rule! {
            name => "has-options-list",
            options_type => Option<Vec<OptionType>>,
            state => {
                [per-run]
                options: Vec<OptionType> = options.unwrap_or_default(),
            },
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    if !self.options.is_empty() {
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
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => [],
                }
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => [{
                        foo => "abc",
                    }],
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_options_default() {
    #[derive(Default, Deserialize)]
    struct Options {
        foo: String,
    }

    RuleTester::run(
        rule! {
            name => "has-options-with-default",
            options_type => Options,
            state => {
                [per-run]
                foo: String = options.foo,
            },
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    if self.foo == "abc" {
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
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => { foo => "def" },
                }
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => {
                        foo => "abc",
                    },
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}

#[test]
fn test_options_variable() {
    #[derive(Default, Deserialize)]
    struct Options {
        foo: String,
    }

    let options = json!({"foo": "abc"});
    RuleTester::run(
        rule! {
            name => "has-options-with-default",
            options_type => Options,
            state => {
                [per-run]
                foo: String = options.foo,
            },
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    if self.foo == "abc" {
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
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => { foo => "def" },
                }
            ],
            invalid => [
                {
                    code => r#"
                        fn whee() {}
                    "#,
                    options => options,
                    errors => [{ message => "whee" }],
                },
            ]
        },
    );
}
