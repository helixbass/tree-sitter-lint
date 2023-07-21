use proc_macros::rule_tests;
use tree_sitter_lint::{no_default_default_rule, RuleTester};

#[test]
fn test_no_default_default_rule() {
    const ERROR_MESSAGE: &str = "Use '_d()' instead of 'Default::default()'";

    RuleTester::run(
        no_default_default_rule(),
        rule_tests! {
            valid => [
                r#"
                    fn foo() {
                        let bar = Default::something_else::default();
                    }
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn foo() {
                            let bar = Default::default();
                        }
                    "#,
                    errors => [ERROR_MESSAGE],
                    output => r#"
                        fn foo() {
                            let bar = _d();
                        }
                    "#,
                },
            ]
        },
    );
}
