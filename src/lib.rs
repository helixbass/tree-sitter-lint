mod args;
mod context;
mod rule;
#[cfg(test)]
mod rule_tester;
mod violation;

use std::{
    borrow::Cow,
    process,
    sync::atomic::{AtomicBool, Ordering},
};

pub use args::Args;
use clap::Parser;
use context::QueryMatchContext;
use rule::{ResolvedRule, Rule, RuleBuilder, RuleListenerBuilder};
use tree_sitter::{Node, Query};
use violation::ViolationBuilder;

use crate::context::Context;

#[macro_export]
macro_rules! regex {
    ($re:expr $(,)?) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

const CAPTURE_NAME_FOR_TREE_SITTER_GREP: &str = "_tree_sitter_lint_capture";
const CAPTURE_NAME_FOR_TREE_SITTER_GREP_WITH_LEADING_AT: &str = "@_tree_sitter_lint_capture";

pub fn run(args: Args) {
    let language = tree_sitter_rust::language();
    let context = Context::new(language);
    let resolved_rules = get_rules()
        .into_iter()
        .filter(|rule| match args.rule.as_ref() {
            Some(rule_arg) => &rule.name == rule_arg,
            None => true,
        })
        .map(|rule| rule.resolve(&context))
        .collect::<Vec<_>>();
    if resolved_rules.is_empty() {
        panic!("Invalid rule name: {:?}", args.rule.as_ref().unwrap());
    }
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &context);
    let tree_sitter_grep_args = tree_sitter_grep::Args::parse_from([
        "tree_sitter_grep",
        "-q",
        &aggregated_queries.query_text,
        "-l",
        "rust",
        "--capture",
        CAPTURE_NAME_FOR_TREE_SITTER_GREP,
    ]);
    let reported_any_violations = AtomicBool::new(false);
    tree_sitter_grep::run_with_callback(
        tree_sitter_grep_args,
        |capture_info, file_contents, path| {
            let (rule_index, rule_listener_index) =
                aggregated_queries.pattern_index_lookup[capture_info.pattern_index];
            let rule = &resolved_rules[rule_index];
            let listener = &rule.listeners[rule_listener_index];
            (listener.on_query_match)(
                capture_info.node,
                &QueryMatchContext::new(
                    path,
                    file_contents,
                    rule,
                    &reported_any_violations,
                    &context,
                ),
            );
        },
    )
    .unwrap();
    if reported_any_violations.load(Ordering::Relaxed) {
        process::exit(1);
    } else {
        process::exit(0);
    }
}

type RuleIndex = usize;
type RuleListenerIndex = usize;

struct AggregatedQueries {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)>,
    #[allow(dead_code)]
    query: Query,
    query_text: String,
}

impl AggregatedQueries {
    pub fn new(resolved_rules: &[ResolvedRule], context: &Context) -> Self {
        let mut pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)> = Default::default();
        let mut aggregated_query_text = String::new();
        for (rule_index, resolved_rule) in resolved_rules.into_iter().enumerate() {
            for (rule_listener_index, rule_listener) in resolved_rule.listeners.iter().enumerate() {
                for _ in 0..rule_listener.query.pattern_count() {
                    pattern_index_lookup.push((rule_index, rule_listener_index));
                }
                let use_capture_name =
                    &rule_listener.query.capture_names()[rule_listener.capture_index as usize];
                let query_text_with_unified_capture_name =
                    regex!(&format!(r#"@{use_capture_name}\b"#)).replace_all(
                        &rule_listener.query_text,
                        CAPTURE_NAME_FOR_TREE_SITTER_GREP_WITH_LEADING_AT,
                    );
                assert!(
                    matches!(query_text_with_unified_capture_name, Cow::Owned(_),),
                    "Didn't find any instances of the capture name to replace"
                );
                aggregated_query_text.push_str(&query_text_with_unified_capture_name);
                aggregated_query_text.push_str("\n\n");
            }
        }
        let query = Query::new(context.language, &aggregated_query_text).unwrap();
        assert!(query.pattern_count() == pattern_index_lookup.len());
        Self {
            pattern_index_lookup,
            query,
            query_text: aggregated_query_text,
        }
    }
}

fn get_rules() -> Vec<Rule> {
    vec![
        no_default_default_rule(),
        no_lazy_static_rule(),
        prefer_impl_param_rule(),
    ]
}

fn no_default_default_rule() -> Rule {
    rule! {
        name => "no_default_default",
        create => |_context| {
            vec![
                rule_listener! {
                    query => r#"(
                      (call_expression
                        function:
                          (scoped_identifier
                            path:
                              (identifier) @first (#eq? @first "Default")
                            name:
                              (identifier) @second (#eq? @second "default")
                          )
                      ) @c
                    )"#,
                    capture_name => "c",
                    on_query_match => |node, query_match_context| {
                        query_match_context.report(
                            ViolationBuilder::default()
                                .message(r#"Use '_d()' instead of 'Default::default()'"#)
                                .node(node)
                                .build()
                                .unwrap(),
                        );
                    }
                }
            ]
        }
    }
}

fn no_lazy_static_rule() -> Rule {
    RuleBuilder::default()
        .name("no_lazy_static")
        .create(|_context| {
            vec![RuleListenerBuilder::default()
                .query(
                    r#"(
                      (macro_invocation
                         macro: (identifier) @c (#eq? @c "lazy_static")
                      )
                    )"#,
                )
                .on_query_match(|node, query_match_context| {
                    query_match_context.report(
                        ViolationBuilder::default()
                            .message(r#"Prefer 'OnceCell::*::Lazy' to 'lazy_static!()'"#)
                            .node(node)
                            .build()
                            .unwrap(),
                    );
                })
                .build()
                .unwrap()]
        })
        .build()
        .unwrap()
}

#[macro_export]
macro_rules! assert_node_kind {
    ($node:expr, $kind:literal $(,)?) => {
        assert_eq!($node.kind(), $kind)
    };
}

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
        match node.parent() {
            None => return None,
            Some(parent) => node = parent,
        }
    }
    Some(node)
}

fn get_ancestor_node_of_kind<'node>(node: Node<'node>, kind: &str) -> Node<'node> {
    maybe_get_ancestor_node_of_kind(node, kind).unwrap()
}

fn get_enclosing_function_node(node: Node) -> Node {
    get_ancestor_node_of_kind(node, "function_item")
}

fn get_parameters_node_of_enclosing_function(node: Node) -> Node {
    let enclosing_function_node = get_enclosing_function_node(node);
    enclosing_function_node
        .child_by_field_name("parameters")
        .unwrap()
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

fn prefer_impl_param_rule() -> Rule {
    rule! {
        name => "prefer_impl_param",
        create => |_context| {
            vec![
                rule_listener! {
                    query => r#"(
                      (function_item
                        type_parameters: (type_parameters
                          (constrained_type_parameter) @c
                        )
                      )
                    )"#,
                    on_query_match => |node, query_match_context| {
                        let type_parameter_name = query_match_context.get_node_text(get_constrained_type_parameter_name(node));
                        return_if_none!(query_match_context.maybe_get_single_matching_node_for_query(
                            &*format!(
                              r#"(
                                (type_identifier) @type_parameter_usage (#eq? @type_parameter_usage "{type_parameter_name}"))"#
                            ),
                            get_parameters_node_of_enclosing_function(node)
                        ));
                        query_match_context.report(
                            ViolationBuilder::default()
                                .message(r#"Prefer using 'param: impl Trait'"#)
                                .node(node)
                                .build()
                                .unwrap(),
                        );
                    }
                }
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macros::rule_tests;

    #[test]
    fn test_prefer_impl_param_rule() {
        use super::*;
        use rule_tester::RuleTester;

        const ERROR_MESSAGE: &str = "Prefer using 'param: impl Trait'";

        RuleTester::run(
            prefer_impl_param_rule(),
            rule_tests! {
                valid => [
                    // no generic parameters
                    r#"
                        fn no_generics(foo: Foo) -> Bar {}
                    "#,
                ],
                invalid => [
                    {
                        code => r#"
                            fn whee<T: Foo>(t: T) -> Bar {}
                        "#,
                        errors => [ERROR_MESSAGE],
                    }
                ]
            },
        );
    }
}
