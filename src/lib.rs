#![allow(clippy::into_iter_on_ref)]

mod cli;
mod config;
mod context;
mod macros;
mod plugin;
mod rule;
mod rule_tester;
#[cfg(test)]
mod tests;
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
pub use cli::bootstrap_cli;
pub use config::{Args, Config, ConfigBuilder};
use context::PendingFix;
pub use context::QueryMatchContext;
pub use plugin::Plugin;
pub use proc_macros::{builder_args, rule, rule_tests};
use rayon::prelude::*;
pub use rule::{FileRunInfo, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta};
use rule::{InstantiatedRule, ResolvedRuleListenerQuery};
pub use rule_tester::{RuleTestInvalid, RuleTester, RuleTests};
use tree_sitter::Query;
use tree_sitter_grep::{CaptureInfo, SupportedLanguage};
pub use violation::ViolationBuilder;
use violation::ViolationWithContext;

pub extern crate clap;
pub extern crate tree_sitter;
pub extern crate tree_sitter_grep;

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
    let instantiated_rules = config.get_instantiated_rules();
    let language = SupportedLanguage::Rust;
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules, language);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let all_violations: Mutex<HashMap<PathBuf, Vec<ViolationWithContext>>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();
    tree_sitter_grep::run_with_per_file_callback(
        tree_sitter_grep_args.clone(),
        |_dir_entry, mut perform_search| {
            let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile>> =
                Default::default();
            perform_search(Box::new(
                |capture_info: CaptureInfo, file_contents, path| {
                    let (instantiated_rule, rule_listener_index) =
                        aggregated_queries.get_rule_and_listener_index(capture_info.pattern_index);
                    let mut query_match_context = QueryMatchContext::new(
                        path,
                        file_contents,
                        instantiated_rule,
                        &config,
                        language,
                    );
                    instantiated_per_file_rules
                        .entry(instantiated_rule.meta.name.clone())
                        .or_insert_with(|| {
                            instantiated_rule
                                .rule_instance
                                .clone()
                                .instantiate_per_file(&FileRunInfo {})
                        })
                        .on_query_match(
                            rule_listener_index,
                            capture_info.node,
                            &mut query_match_context,
                        );
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
                        files_with_fixes.append(
                            path,
                            file_contents,
                            &instantiated_rule.meta.name,
                            fixes,
                        );
                    }
                },
            ));
        },
    )
    .unwrap();
    let mut all_violations = all_violations.into_inner().unwrap();
    if !config.fix {
        return all_violations.into_values().flatten().collect();
    }
    let files_with_fixes = files_with_fixes.into_inner().unwrap();
    if files_with_fixes.is_empty() {
        return all_violations.into_values().flatten().collect();
    }
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
                    language,
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

#[allow(clippy::too_many_arguments)]
fn run_fixing_loop(
    violations: &mut Vec<ViolationWithContext>,
    file_contents: &mut Vec<u8>,
    mut pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
    tree_sitter_grep_args: tree_sitter_grep::Args,
    aggregated_queries: &AggregatedQueries,
    path: &Path,
    config: &Config,
    language: SupportedLanguage,
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
        let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile>> =
            Default::default();
        tree_sitter_grep::run_for_slice_with_callback(
            file_contents,
            tree_sitter_grep_args.clone(),
            |capture_info| {
                let (instantiated_rule, rule_listener_index) =
                    aggregated_queries.get_rule_and_listener_index(capture_info.pattern_index);
                let mut query_match_context = QueryMatchContext::new(
                    path,
                    file_contents,
                    instantiated_rule,
                    config,
                    language,
                );
                instantiated_per_file_rules
                    .entry(instantiated_rule.meta.name.clone())
                    .or_insert_with(|| {
                        instantiated_rule
                            .rule_instance
                            .clone()
                            .instantiate_per_file(&FileRunInfo {})
                    })
                    .on_query_match(
                        rule_listener_index,
                        capture_info.node,
                        &mut query_match_context,
                    );
                if let Some(reported_violations) = query_match_context.violations.take() {
                    violations.extend(reported_violations);
                }
                if let Some(fixes) = query_match_context.into_pending_fixes() {
                    pending_fixes
                        .entry(instantiated_rule.meta.name.clone())
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
    let instantiated_rules = config.get_instantiated_rules();
    let language = SupportedLanguage::Rust;
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules, language);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile>> =
        Default::default();
    tree_sitter_grep::run_for_slice_with_callback(
        file_contents,
        tree_sitter_grep_args,
        |capture_info| {
            let (instantiated_rule, rule_listener_index) =
                aggregated_queries.get_rule_and_listener_index(capture_info.pattern_index);
            let mut query_match_context =
                QueryMatchContext::new(path, file_contents, instantiated_rule, &config, language);
            instantiated_per_file_rules
                .entry(instantiated_rule.meta.name.clone())
                .or_insert_with(|| {
                    instantiated_rule
                        .rule_instance
                        .clone()
                        .instantiate_per_file(&FileRunInfo {})
                })
                .on_query_match(
                    rule_listener_index,
                    capture_info.node,
                    &mut query_match_context,
                );
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
    let instantiated_rules = config.get_instantiated_rules();
    let language = SupportedLanguage::Rust;
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules, language);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let pending_fixes: Mutex<HashMap<RuleName, Vec<PendingFix>>> = Default::default();
    let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile>> =
        Default::default();
    tree_sitter_grep::run_for_slice_with_callback(
        file_contents,
        tree_sitter_grep_args.clone(),
        |capture_info| {
            let (instantiated_rule, rule_listener_index) =
                aggregated_queries.get_rule_and_listener_index(capture_info.pattern_index);
            let mut query_match_context =
                QueryMatchContext::new(path, file_contents, instantiated_rule, &config, language);
            instantiated_per_file_rules
                .entry(instantiated_rule.meta.name.clone())
                .or_insert_with(|| {
                    instantiated_rule
                        .rule_instance
                        .clone()
                        .instantiate_per_file(&FileRunInfo {})
                })
                .on_query_match(
                    rule_listener_index,
                    capture_info.node,
                    &mut query_match_context,
                );
            if let Some(reported_violations) = query_match_context.violations.take() {
                violations.lock().unwrap().extend(reported_violations);
            }
            if let Some(fixes) = query_match_context.into_pending_fixes() {
                pending_fixes
                    .lock()
                    .unwrap()
                    .entry(instantiated_rule.meta.name.clone())
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
        language,
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

struct AggregatedQueries<'a> {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)>,
    #[allow(dead_code)]
    query: Query,
    query_text: String,
    instantiated_rules: &'a [InstantiatedRule],
}

impl<'a> AggregatedQueries<'a> {
    pub fn new(instantiated_rules: &'a [InstantiatedRule], language: SupportedLanguage) -> Self {
        let mut pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)> = Default::default();
        let mut aggregated_query_text = String::new();
        let language = language.language();
        for (rule_index, instantiated_rule) in instantiated_rules.into_iter().enumerate() {
            for (rule_listener_index, rule_listener_query) in instantiated_rule
                .rule_instance
                .listener_queries()
                .iter()
                .map(|rule_listener_query| rule_listener_query.resolve(language))
                .enumerate()
            {
                for _ in 0..rule_listener_query.query.pattern_count() {
                    pattern_index_lookup.push((rule_index, rule_listener_index));
                }
                let query_text_with_unified_capture_name =
                    regex!(&format!(r#"@{}\b"#, rule_listener_query.capture_name())).replace_all(
                        &rule_listener_query.query_text,
                        CAPTURE_NAME_FOR_TREE_SITTER_GREP_WITH_LEADING_AT,
                    );
                assert!(were_any_captures_replaced(
                    &query_text_with_unified_capture_name,
                    &rule_listener_query
                ));
                aggregated_query_text.push_str(&query_text_with_unified_capture_name);
                aggregated_query_text.push_str("\n\n");
            }
        }
        let query = Query::new(language, &aggregated_query_text).unwrap();
        assert!(query.pattern_count() == pattern_index_lookup.len());
        Self {
            pattern_index_lookup,
            query,
            query_text: aggregated_query_text,
            instantiated_rules,
        }
    }

    pub fn get_rule_and_listener_index(
        &self,
        pattern_index: usize,
    ) -> (&'a InstantiatedRule, usize) {
        let (rule_index, rule_listener_index) = self.pattern_index_lookup[pattern_index];
        let instantiated_rule = &self.instantiated_rules[rule_index];
        (instantiated_rule, rule_listener_index)
    }
}

#[allow(clippy::ptr_arg)]
fn were_any_captures_replaced(
    query_text_with_unified_capture_name: &Cow<'_, str>,
    _rule_listener: &ResolvedRuleListenerQuery,
) -> bool {
    // It's a presumed invariant of `Regex::replace_all()` that it returns a
    // `Cow::Owned` iff it made any modifications to the original `&str` that it was
    // passed
    matches!(query_text_with_unified_capture_name, Cow::Owned(_))
}
