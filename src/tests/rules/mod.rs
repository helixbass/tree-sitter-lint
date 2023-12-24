#![cfg(test)]

use std::{
    mem,
    sync::{Arc, OnceLock},
};

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use tree_sitter_grep::RopeOrSlice;

mod options;
mod state;
mod tokens;
mod violations;

use crate::{
    rule::Rule, FileRunContext, FromFileRunContext, FromFileRunContextInstanceProvider,
    FromFileRunContextInstanceProviderFactory, RuleTester, ROOT_EXIT,
};

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

fn no_more_than_n_uses_of_foo_rule<
    TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
>() -> Arc<dyn Rule<TFromFileRunContextInstanceProviderFactory>> {
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
fn test_root_exit_listener() {
    RuleTester::run(
        rule! {
            name => "uses-root-exit-listener",
            listeners => [
                ROOT_EXIT => |node, context| {
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
                ROOT_EXIT => |node, context| {
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
fn test_retrieve() {
    use better_any::{tid, Tid, TidAble};

    #[derive(Clone)]
    struct Foo<'a> {
        #[allow(dead_code)]
        text: &'a str,
    }

    impl<'a> FromFileRunContext<'a> for Foo<'a> {
        fn from_file_run_context(
            file_run_context: FileRunContext<
                'a,
                '_,
                impl FromFileRunContextInstanceProviderFactory,
            >,
        ) -> Self {
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

    #[derive(Default)]
    struct FooProvider<'a> {
        foo_instance: OnceLock<Foo<'a>>,
    }

    impl<'a> FromFileRunContextInstanceProvider<'a> for FooProvider<'a> {
        type Parent = FooProviderFactory;

        fn get<T: FromFileRunContext<'a> + TidAble<'a>>(
            &self,
            file_run_context: FileRunContext<'a, '_, Self::Parent>,
        ) -> Option<&T> {
            match T::id() {
                id if id == Foo::<'a>::id() => Some(unsafe {
                    mem::transmute::<&Foo<'a>, &T>(
                        self.foo_instance
                            .get_or_init(|| Foo::from_file_run_context(file_run_context)),
                    )
                }),
                _ => None,
            }
        }
    }

    struct FooProviderFactory;

    impl FromFileRunContextInstanceProviderFactory for FooProviderFactory {
        type Provider<'a> = FooProvider<'a>;

        fn create<'a>(&self) -> Self::Provider<'a> {
            FooProvider {
                foo_instance: Default::default(),
            }
        }
    }

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
        FooProviderFactory,
    );
}
