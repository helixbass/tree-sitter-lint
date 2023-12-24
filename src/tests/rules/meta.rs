use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};

use crate::RuleTester;

#[test]
fn test_concatenate_adjacent_insert_fixes() {
    RuleTester::run(
        rule! {
            name => "concatenate-adjacent-insert-fixes",
            fixable => true,
            concatenate_adjacent_insert_fixes => true,
            listeners => [
                r#"(
                  (function_item) @c
                )"# => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "whee",
                        fix => |fixer| {
                            fixer.insert_text_before(node, "use foo::bar;\n");
                        }
                    });
                    context.report(violation! {
                        node => node,
                        message => "whee",
                        fix => |fixer| {
                            fixer.insert_text_before(node, "use bar::baz;\n");
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
                    code => "\
fn foo() {}
                    ",
                    output => "\
use foo::bar;
use bar::baz;
fn foo() {}
                    ",
                    errors => 2,
                },
            ]
        },
    );
}
