use std::sync::Arc;

use tracing::{instrument, trace_span};
use tree_sitter_lint::{rule, tree_sitter::Node, violation, Rule};

#[macro_export]
macro_rules! assert_node_kind {
    ($node:expr, $kind:literal $(,)?) => {{
        assert_eq!($node.kind(), $kind);
        $node
    }};
}

#[instrument(level = "trace")]
fn get_constrained_type_parameter_name(node: Node) -> Node {
    assert_node_kind!(node, "constrained_type_parameter");
    let name_node = node.child_by_field_name("left").unwrap();
    assert_node_kind!(name_node, "type_identifier");
    name_node
}

fn maybe_get_ancestor_node_of_kind<'node>(
    mut node: Node<'node>,
    kind: &str,
) -> Option<Node<'node>> {
    while node.kind() != kind {
        node = node.parent()?;
    }
    Some(node)
}

#[allow(dead_code)]
fn get_ancestor_node_of_kind<'node>(node: Node<'node>, kind: &str) -> Node<'node> {
    maybe_get_ancestor_node_of_kind(node, kind).unwrap()
}

fn get_enclosing_function_node(node: Node) -> Node {
    maybe_get_enclosing_function_node(node).unwrap()
}

fn maybe_get_enclosing_function_node(node: Node) -> Option<Node> {
    maybe_get_ancestor_node_of_kind(node, "function_item")
}

fn get_parameters_node_of_enclosing_function(node: Node) -> Node {
    let enclosing_function_node = get_enclosing_function_node(node);
    enclosing_function_node
        .child_by_field_name("parameters")
        .unwrap()
}

fn maybe_get_return_type_node_of_enclosing_function(node: Node) -> Option<Node> {
    let enclosing_function_node = maybe_get_enclosing_function_node(node)?;
    enclosing_function_node.child_by_field_name("return_type")
}

fn maybe_first_child_of_kind<'node>(node: Node<'node>, kind: &str) -> Option<Node<'node>> {
    let mut tree_cursor = node.walk();
    let ret = node
        .children(&mut tree_cursor)
        .find(|child| child.kind() == kind);
    ret
}

fn maybe_get_where_clause_node_of_enclosing_function(node: Node) -> Option<Node> {
    let enclosing_function_node = maybe_get_enclosing_function_node(node)?;
    maybe_first_child_of_kind(enclosing_function_node, "where_clause")
}

#[macro_export]
macro_rules! return_if_none {
    ($expr:expr $(,)?) => {
        match $expr {
            None => {
                return;
            }
            Some(expr) => expr,
        }
    };
}

#[macro_export]
macro_rules! query {
    ($query:literal, $language:expr $(,)?) => {{
        static CACHED: std::sync::OnceLock<tree_sitter_lint::tree_sitter::Query> =
            std::sync::OnceLock::new();
        CACHED.get_or_init(|| tree_sitter_lint::tree_sitter::Query::new($language, $query).unwrap())
    }};
}

pub fn prefer_impl_param_rule() -> Arc<dyn Rule> {
    rule! {
        name => "prefer-impl-param",
        listeners => [
            r#"(
              (function_item
                type_parameters: (type_parameters
                  (constrained_type_parameter) @c
                )
              )
            )"# => |node, context| {
                let type_parameter_name =
                    context.get_node_text(get_constrained_type_parameter_name(node));

                let span = trace_span!("get type identifier query").entered();

                let type_identifier_query =
                    query!(
                        r#"(type_identifier) @c"#,
                        context.language().language(),
                    );

                span.exit();

                let span = trace_span!("look for single usage in parameters").entered();

                return_if_none!(context.maybe_get_single_captured_node_for_filtered_query(
                    type_identifier_query,
                    |node| context.get_node_text(node) == type_parameter_name,
                    get_parameters_node_of_enclosing_function(node)
                ));

                span.exit();

                let span = trace_span!("look for usage in return type").entered();

                if let Some(return_type_node) =
                    maybe_get_return_type_node_of_enclosing_function(node)
                {
                    if return_type_node.kind() == "type_identifier"
                        && context.get_node_text(return_type_node) == type_parameter_name
                    {
                        return;
                    }
                    if context.get_number_of_filtered_query_captures(
                        type_identifier_query,
                        |node| context.get_node_text(node) == type_parameter_name,
                        return_type_node
                    ) > 0 {
                        return;
                    }
                }

                span.exit();

                let type_parameters_node =
                    assert_node_kind!(node.parent().unwrap(), "type_parameters");

                let span = trace_span!("looking for other usages in type parameters").entered();

                let only_found_the_defining_usage_in_the_type_parameters = context.maybe_get_single_captured_node_for_filtered_query(
                    type_identifier_query,
                    |node| context.get_node_text(node) == type_parameter_name,
                    type_parameters_node
                ).is_some();

                span.exit();

                if !only_found_the_defining_usage_in_the_type_parameters {
                    return;
                }

                let span = trace_span!("looking in where clause").entered();

                if let Some(where_clause_node) =
                    maybe_get_where_clause_node_of_enclosing_function(node)
                {
                    if context.get_number_of_filtered_query_captures(
                        type_identifier_query,
                        |node| context.get_node_text(node) == type_parameter_name,
                        where_clause_node
                    ) > 0 {
                        return;
                    }
                }

                span.exit();

                context.report(
                    violation! {
                        message => r#"Prefer using 'param: impl Trait'"#,
                        node => node,
                    }
                );
            }
        ],
        languages => [Rust],
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter_lint::{rule_tests, RuleTester};

    use super::*;

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
                    // generic param is return type
                    r#"
                        fn foo<T: Foo>(foo: T) -> T {}
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
}
