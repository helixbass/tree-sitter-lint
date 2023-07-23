#![cfg(test)]

use std::sync::Arc;

use proc_macros::{rule, rule_tests};

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
