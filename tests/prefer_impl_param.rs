use proc_macros::rule_tests;
use tree_sitter_lint::{prefer_impl_param_rule, RuleTester};

#[test]
fn test_prefer_impl_param_rule() {
    const ERROR_MESSAGE: &str = "Prefer using 'param: impl Trait'";

    RuleTester::run(
        prefer_impl_param_rule(),
        rule_tests! {
            valid => [
                // no generic parameters
                r#"
                    fn foo(foo: Foo) -> Bar {}
                "#,
                // generic param used in multiple places
                r#"
                    fn foo<T: Foo>(foo: T, bar: T) -> Bar {}
                "#,
                // generic param used in return type
                r#"
                    fn foo<T: Foo>(foo: T) -> Bar<T> {}
                "#,
                // unconstrained generic
                r#"
                    fn foo<T>(foo: T) -> Bar {}
                "#,
                // used in onother generic constraint as well as in a param type
                r#"
                    fn foo<T: Foo, U: IntoIterator<Item = T>>(foo: T, bar: U) -> Bar<U> {}
                "#,
                // used in where clause as well as in a param type
                r#"
                    fn foo<T: Foo, U>(foo: T, bar: U) -> Bar<U>
                        where U: IntoIterator<Item = T> {}
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn whee<T: Foo>(t: T) -> Bar {}
                    "#,
                    errors => [ERROR_MESSAGE],
                },
                {
                    // used in nested generic
                    code => r#"
                        fn whee<T: Foo>(t: Rc<T>) -> Bar {}
                    "#,
                    errors => [ERROR_MESSAGE],
                },
            ]
        },
    );
}
