use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

use crate::RuleTester;

#[test]
fn test_rule_methods() {
    RuleTester::run(
        rule! {
            name => "has-methods",
            state => {
                [per-config]
                foo: bool = true,
            },
            methods => {
                fn is_foo(&self) -> bool {
                    self.foo
                }
            },
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    if self.is_foo() {
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
                    code => "fn foo() {}",
                    errors => 1,
                },
            ]
        },
    );
}

#[test]
fn test_rule_method_mut_self() {
    RuleTester::run(
        rule! {
            name => "has-methods",
            state => {
                [per-file-run]
                foo: bool = false,
            },
            methods => {
                fn set_foo(&mut self) {
                    self.foo = true;
                }
            },
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    self.set_foo();
                    if self.foo {
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
                    code => "fn foo() {}",
                    errors => 1,
                },
            ]
        },
    );
}

#[test]
fn test_rule_methods_multiple() {
    RuleTester::run(
        rule! {
            name => "has-methods",
            state => {
                [per-file-run]
                foo: bool = false,
            },
            methods => {
                fn set_foo_to(&mut self, to: bool) {
                    self.actually_set_foo_to(to);
                }

                fn actually_set_foo_to(&mut self, to: bool) {
                    self.foo = to;
                }
            },
            listeners => [
                r#"
                  (function_item) @c
                "# => |node, context| {
                    self.set_foo_to(true);
                    if self.foo {
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
                    code => "fn foo() {}",
                    errors => 1,
                },
            ]
        },
    );
}
