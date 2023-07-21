#![allow(clippy::into_iter_on_ref)]
mod args;
mod context;
mod rule;
mod rule_tester;
mod rules;
mod violation;

use std::{
    borrow::Cow,
    process,
    sync::atomic::{AtomicBool, Ordering},
};

pub use args::Args;
use clap::Parser;
use context::QueryMatchContext;
use rule::{ResolvedRule, Rule};
pub use rule_tester::{RuleTestInvalid, RuleTester, RuleTests};
use tree_sitter::Query;
use violation::ViolationBuilder;

use crate::context::Context;
pub use crate::rules::{no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule};

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
