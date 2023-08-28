use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use serde::Deserialize;
use tree_sitter_grep::RopeOrSlice;

use crate::{FileRunContext, FromFileRunContext, RuleTester};

#[test]
fn test_retrieve() {
    use better_any::tid;
    use proc_macros::instance_provider_factory_crate_internal as instance_provider_factory;

    #[derive(Clone)]
    struct Foo<'a> {
        #[allow(dead_code)]
        text: &'a str,
    }

    impl<'a> FromFileRunContext<'a> for Foo<'a> {
        fn from_file_run_context(file_run_context: FileRunContext<'a, '_>) -> Self {
            Self {
                text: match &file_run_context.file_contents {
                    RopeOrSlice::Slice(file_contents) => {
                        std::str::from_utf8(&file_contents[..4]).unwrap()
                    }
                    _ => unreachable!(),
                },
            }
        }
    }

    tid! { impl<'a> TidAble<'a> for Foo<'a> }

    type ProvidedTypes<'a> = (Foo<'a>,);

    RuleTester::run_with_from_file_run_context_instance_provider(
        rule! {
            name => "uses-retrieve",
            listeners => [
                r#"(
                  (use_declaration) @c
                )"# => |node, context| {
                    if context.retrieve::<Foo<'a>>().text == "use " {
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
                r#"fn whee() {}"#,
            ],
            invalid => [
                {
                    code => r#"use foo::bar;"#,
                    errors => [{ message => "whee" }],
                },
            ]
        },
        Box::new(instance_provider_factory!(ProvidedTypes)),
    );
}

#[test]
fn test_has_visibility_of_environment() {
    use better_any::tid;
    use proc_macros::instance_provider_factory_crate_internal as instance_provider_factory;

    #[derive(Clone, Debug, Default, Deserialize)]
    struct Foo {
        field: String,
    }

    impl<'a> FromFileRunContext<'a> for Foo {
        fn from_file_run_context(file_run_context: FileRunContext<'a, '_>) -> Self {
            serde_json::from_value(serde_json::Value::Object(
                file_run_context.environment.clone(),
            ))
            .unwrap_or_default()
        }
    }

    tid! { impl<'a> TidAble<'a> for Foo }

    type ProvidedTypes<'a> = (Foo,);

    RuleTester::run_with_from_file_run_context_instance_provider(
        rule! {
            name => "uses-environment",
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    if context.retrieve::<Foo>().field == "bar" {
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
                r#"fn whee() {}"#,
                {
                    code => r#"fn whee() {}"#,
                    environment => {
                        field => "baz",
                    },
                }
            ],
            invalid => [
                {
                    code => r#"fn whee() {}"#,
                    environment => {
                        field => "bar",
                    },
                    errors => [{ message => "whee" }],
                },
            ]
        },
        Box::new(instance_provider_factory!(ProvidedTypes)),
    );
}
