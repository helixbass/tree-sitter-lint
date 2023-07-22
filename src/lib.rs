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
    cmp::Ordering,
    collections::HashMap,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process,
    sync::{Mutex, PoisonError},
};

use clap::Parser;
pub use config::Config;
use context::{PendingFix, QueryMatchContext};
use rayon::prelude::*;
use rule::{ResolvedRule, ResolvedRuleListener};
pub use rule_tester::{RuleTestInvalid, RuleTester, RuleTests};
use tree_sitter::Query;
use violation::{ViolationBuilder, ViolationWithContext};

const CAPTURE_NAME_FOR_TREE_SITTER_GREP: &str = "_tree_sitter_lint_capture";
const CAPTURE_NAME_FOR_TREE_SITTER_GREP_WITH_LEADING_AT: &str = "@_tree_sitter_lint_capture";

pub fn run_and_output(config: Config) {
    let violations = run(config);
    if violations.is_empty() {
        process::exit(0);
    }
    for violation in violations {
        violation.print();
    }
    process::exit(1);
}

const MAX_FIX_ITERATIONS: usize = 10;

pub fn run(config: Config) -> Vec<ViolationWithContext> {
    let resolved_rules = config.get_resolved_rules();
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &config);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let all_violations: Mutex<HashMap<PathBuf, Vec<ViolationWithContext>>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();
    tree_sitter_grep::run_with_callback(
        tree_sitter_grep_args.clone(),
        |capture_info, file_contents, path| {
            let (rule, rule_listener) =
                aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
            let mut query_match_context =
                QueryMatchContext::new(path, file_contents, rule, &config);
            (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
            if let Some(violations) = query_match_context.violations.take() {
                all_violations
                    .lock()
                    .unwrap()
                    .entry(path.to_owned())
                    .or_default()
                    .extend(violations);
            }
            if let Some(fixes) = query_match_context.into_pending_fixes() {
                assert!(config.fix);
                files_with_fixes.append(path, file_contents, &rule.meta.name, fixes);
            }
        },
    )
    .unwrap();
    let mut all_violations = all_violations.into_inner().unwrap();
    if !config.fix {
        return all_violations.into_values().flatten().collect();
    }
    let files_with_fixes = files_with_fixes.into_inner().unwrap();
    let aggregated_results_from_files_with_fixes: HashMap<
        PathBuf,
        (Vec<u8>, Vec<ViolationWithContext>),
    > = files_with_fixes
        .into_par_iter()
        .map(
            |(
                path,
                PerFilePendingFixes {
                    mut file_contents,
                    pending_fixes,
                },
            )| {
                let mut violations: Vec<ViolationWithContext> = Default::default();
                run_fixing_loop(
                    &mut violations,
                    &mut file_contents,
                    pending_fixes,
                    tree_sitter_grep_args.clone(),
                    &aggregated_queries,
                    &path,
                    &config,
                );
                (path, (file_contents, violations))
            },
        )
        .collect();
    write_files(
        aggregated_results_from_files_with_fixes
            .iter()
            .map(|(path, (file_contents, _))| (&**path, &**file_contents)),
    );
    for (path, (_, violations)) in aggregated_results_from_files_with_fixes {
        all_violations.insert(path, violations);
    }
    all_violations.into_values().flatten().collect()
}

fn run_fixing_loop(
    violations: &mut Vec<ViolationWithContext>,
    file_contents: &mut Vec<u8>,
    mut pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
    tree_sitter_grep_args: tree_sitter_grep::Args,
    aggregated_queries: &AggregatedQueries,
    path: &Path,
    config: &Config,
) {
    for _ in 0..MAX_FIX_ITERATIONS {
        apply_fixes(file_contents, pending_fixes);
        pending_fixes = Default::default();
        if config.report_fixed_violations {
            *violations = violations
                .iter()
                .filter(|violation| violation.was_fix)
                .cloned()
                .collect();
        } else {
            violations.clear();
        }
        tree_sitter_grep::run_for_slice_with_callback(
            file_contents,
            tree_sitter_grep_args.clone(),
            |capture_info| {
                let (rule, rule_listener) =
                    aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
                let mut query_match_context =
                    QueryMatchContext::new(path, file_contents, rule, config);
                (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
                if let Some(reported_violations) = query_match_context.violations.take() {
                    violations.extend(reported_violations);
                }
                if let Some(fixes) = query_match_context.into_pending_fixes() {
                    pending_fixes
                        .entry(rule.meta.name.clone())
                        .or_default()
                        .extend(fixes);
                }
            },
        )
        .unwrap();
        if pending_fixes.is_empty() {
            break;
        }
    }
}

pub fn run_for_slice(
    file_contents: &[u8],
    path: impl AsRef<Path>,
    config: Config,
) -> Vec<ViolationWithContext> {
    let path = path.as_ref();
    if config.fix {
        panic!("Use run_fixing_for_slice()");
    }
    let resolved_rules = config.get_resolved_rules();
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &config);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    tree_sitter_grep::run_for_slice_with_callback(
        file_contents,
        tree_sitter_grep_args,
        |capture_info| {
            let (rule, rule_listener) =
                aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
            let mut query_match_context =
                QueryMatchContext::new(path, file_contents, rule, &config);
            (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
            if let Some(reported_violations) = query_match_context.violations.take() {
                violations.lock().unwrap().extend(reported_violations);
            }
            assert!(query_match_context.pending_fixes().is_none());
        },
    )
    .unwrap();
    violations.into_inner().unwrap()
}

pub fn run_fixing_for_slice(
    file_contents: &mut Vec<u8>,
    path: impl AsRef<Path>,
    config: Config,
) -> Vec<ViolationWithContext> {
    let path = path.as_ref();
    if !config.fix {
        panic!("Use run_for_slice()");
    }
    let resolved_rules = config.get_resolved_rules();
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &config);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let pending_fixes: Mutex<HashMap<RuleName, Vec<PendingFix>>> = Default::default();
    tree_sitter_grep::run_for_slice_with_callback(
        file_contents,
        tree_sitter_grep_args.clone(),
        |capture_info| {
            let (rule, rule_listener) =
                aggregated_queries.get_rule_and_listener(capture_info.pattern_index);
            let mut query_match_context =
                QueryMatchContext::new(path, file_contents, rule, &config);
            (rule_listener.on_query_match)(capture_info.node, &mut query_match_context);
            if let Some(reported_violations) = query_match_context.violations.take() {
                violations.lock().unwrap().extend(reported_violations);
            }
            if let Some(fixes) = query_match_context.into_pending_fixes() {
                pending_fixes
                    .lock()
                    .unwrap()
                    .entry(rule.meta.name.clone())
                    .or_default()
                    .extend(fixes);
            }
        },
    )
    .unwrap();
    let mut violations = violations.into_inner().unwrap();
    let pending_fixes = pending_fixes.into_inner().unwrap();
    if pending_fixes.is_empty() {
        return violations;
    }
    run_fixing_loop(
        &mut violations,
        file_contents,
        pending_fixes,
        tree_sitter_grep_args,
        &aggregated_queries,
        path,
        &config,
    );
    violations
}

fn get_tree_sitter_grep_args(aggregated_queries: &AggregatedQueries) -> tree_sitter_grep::Args {
    tree_sitter_grep::Args::parse_from([
        "tree_sitter_grep",
        "-q",
        &aggregated_queries.query_text,
        "-l",
        "rust",
        "--capture",
        CAPTURE_NAME_FOR_TREE_SITTER_GREP,
    ])
}

fn write_files<'a>(files_to_write: impl Iterator<Item = (&'a Path, &'a [u8])>) {
    for (path, file_contents) in files_to_write {
        fs::write(path, file_contents).unwrap();
    }
}

type RuleName = String;

fn apply_fixes(file_contents: &mut Vec<u8>, pending_fixes: HashMap<RuleName, Vec<PendingFix>>) {
    let non_conflicting_sorted_pending_fixes =
        get_sorted_non_conflicting_pending_fixes(pending_fixes);
    for PendingFix { range, replacement } in non_conflicting_sorted_pending_fixes.into_iter().rev()
    {
        file_contents.splice(range, replacement.into_bytes());
    }
}

fn compare_pending_fixes(a: &PendingFix, b: &PendingFix) -> Ordering {
    if a.range.start < b.range.start {
        return Ordering::Less;
    }
    if a.range.start > b.range.start {
        return Ordering::Greater;
    }
    if a.range.end < b.range.end {
        return Ordering::Less;
    }
    if a.range.end > b.range.end {
        return Ordering::Greater;
    }
    Ordering::Equal
}

fn has_overlapping_ranges(sorted_pending_fixes: &[PendingFix]) -> bool {
    let mut prev_end = None;
    for pending_fix in sorted_pending_fixes {
        if let Some(prev_end) = prev_end {
            if pending_fix.range.start < prev_end {
                return true;
            }
        }
        prev_end = Some(pending_fix.range.end);
    }
    false
}

fn get_sorted_non_conflicting_pending_fixes(
    pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
) -> Vec<PendingFix> {
    pending_fixes.into_iter().fold(
        Default::default(),
        |accumulated_fixes, (rule_name, mut pending_fixes_for_rule)| {
            pending_fixes_for_rule.sort_by(compare_pending_fixes);
            if has_overlapping_ranges(&pending_fixes_for_rule) {
                panic!("Rule {:?} tried to apply self-conflicting fixes", rule_name);
            }
            let mut tentative = accumulated_fixes.clone();
            tentative.extend(pending_fixes_for_rule);
            if has_overlapping_ranges(&tentative) {
                accumulated_fixes
            } else {
                tentative
            }
        },
    )
}

#[derive(Default)]
struct AllPendingFixes(Mutex<HashMap<PathBuf, PerFilePendingFixes>>);

impl AllPendingFixes {
    pub fn append(
        &self,
        path: &Path,
        file_contents: &[u8],
        rule_name: &str,
        fixes: Vec<PendingFix>,
    ) {
        self.lock()
            .unwrap()
            .entry(path.to_owned())
            .or_insert_with(|| PerFilePendingFixes::new(file_contents.to_owned()))
            .pending_fixes
            .entry(rule_name.to_owned())
            .or_default()
            .extend(fixes);
    }

    pub fn into_inner(
        self,
    ) -> Result<
        HashMap<PathBuf, PerFilePendingFixes>,
        PoisonError<HashMap<PathBuf, PerFilePendingFixes>>,
    > {
        self.0.into_inner()
    }
}

impl Deref for AllPendingFixes {
    type Target = Mutex<HashMap<PathBuf, PerFilePendingFixes>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct PerFilePendingFixes {
    file_contents: Vec<u8>,
    pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
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
