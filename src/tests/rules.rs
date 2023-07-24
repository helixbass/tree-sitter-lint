#![cfg(test)]

use std::sync::Arc;

use proc_macros::{rule_crate_internal as rule, rule_tests_crate_internal as rule_tests};

use crate::{rule::Rule, violation, RuleTester};

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
        ]
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
    // parse_options => |options| -> usize {
    //     if options.len() != 1 {
    //         return Err("Expected single option for # of uses");
    //     }
    // }
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
        ]
    }
}
