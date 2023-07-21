#![allow(clippy::into_iter_on_ref)]
mod config;
mod context;
mod macros;
mod rule;
mod rule_tester;
mod rules;
mod violation;

use std::{
    borrow::Cow,
    collections::HashMap,
    ops::Deref,
    path::PathBuf,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use clap::Parser;
pub use config::Config;
use context::{PendingFix, QueryMatchContext};
use rule::{ResolvedRule, ResolvedRuleListener, Rule};
pub use rule_tester::{RuleTestInvalid, RuleTester, RuleTests};
use tree_sitter::Query;
use violation::ViolationBuilder;

pub use crate::rules::{no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule};

const CAPTURE_NAME_FOR_TREE_SITTER_GREP: &str = "_tree_sitter_lint_capture";
const CAPTURE_NAME_FOR_TREE_SITTER_GREP_WITH_LEADING_AT: &str = "@_tree_sitter_lint_capture";

pub fn run(config: Config) {
    let resolved_rules = get_rules()
        .into_iter()
        .filter(|rule| match config.rule.as_ref() {
            Some(rule_arg) => &rule.meta.name == rule_arg,
            None => true,
        })
        .map(|rule| rule.resolve(&config))
        .collect::<Vec<_>>();
    if resolved_rules.is_empty() {
        panic!("Invalid rule name: {:?}", config.rule.as_ref().unwrap());
    }
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &config);
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
    if config.fix {
        let files_with_fixes: AllPendingFixes = Default::default();
        tree_sitter_grep::run_with_callback(
            tree_sitter_grep_args,
            |capture_info, file_contents, path| {
                let (rule, rule_listener) =
                    aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
                let mut query_match_context = QueryMatchContext::new(
                    path,
                    file_contents,
                    rule,
                    &reported_any_violations,
                    &config,
                );
                (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
                if let Some(fixes) = query_match_context.into_pending_fixes() {
                    files_with_fixes
                        .lock()
                        .unwrap()
                        .entry(path.to_owned())
                        .or_insert_with(|| PerFilePendingFixes::new(file_contents.to_owned()))
                        .pending_fixes
                        .extend(fixes);
                }
            },
        )
        .unwrap();
        // if files_with_fixes.has_any_
    } else {
        tree_sitter_grep::run_with_callback(
            tree_sitter_grep_args,
            |capture_info, file_contents, path| {
                let (rule, rule_listener) =
                    aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
                let mut query_match_context = QueryMatchContext::new(
                    path,
                    file_contents,
                    rule,
                    &reported_any_violations,
                    &config,
                );
                (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
                assert!(query_match_context.into_pending_fixes().is_none());
            },
        )
        .unwrap();
    }
    if reported_any_violations.load(Ordering::Relaxed) {
        process::exit(1);
    } else {
        process::exit(0);
    }
}

#[derive(Default)]
struct AllPendingFixes(Mutex<HashMap<PathBuf, PerFilePendingFixes>>);

impl Deref for AllPendingFixes {
    type Target = Mutex<HashMap<PathBuf, PerFilePendingFixes>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct PerFilePendingFixes {
    file_contents: Vec<u8>,
    pending_fixes: Vec<PendingFix>,
}

impl PerFilePendingFixes {
    fn new(file_contents: Vec<u8>) -> Self {
        Self {
            file_contents,
            pending_fixes: Default::default(),
        }
    }
}

type RuleIndex = usize;
type RuleListenerIndex = usize;

struct AggregatedQueries<'resolved_rules> {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)>,
    #[allow(dead_code)]
    query: Query,
    query_text: String,
    resolved_rules: &'resolved_rules [ResolvedRule<'resolved_rules>],
}

impl<'resolved_rules> AggregatedQueries<'resolved_rules> {
    pub fn new(
        resolved_rules: &'resolved_rules [ResolvedRule<'resolved_rules>],
        config: &Config,
    ) -> Self {
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
        let query = Query::new(config.language.language(), &aggregated_query_text).unwrap();
        assert!(query.pattern_count() == pattern_index_lookup.len());
        Self {
            pattern_index_lookup,
            query,
            query_text: aggregated_query_text,
            resolved_rules,
        }
    }

    pub fn get_rule_and_listener(
        &self,
        pattern_index: usize,
    ) -> (
        &'resolved_rules ResolvedRule<'resolved_rules>,
        &'resolved_rules ResolvedRuleListener,
    ) {
        let (rule_index, rule_listener_index) = self.pattern_index_lookup[pattern_index];
        let rule = &self.resolved_rules[rule_index];
        (rule, &rule.listeners[rule_listener_index])
    }
}

fn get_rules() -> Vec<Rule> {
    vec![
        no_default_default_rule(),
        no_lazy_static_rule(),
        prefer_impl_param_rule(),
    ]
}
