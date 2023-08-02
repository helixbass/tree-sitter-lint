#![allow(clippy::into_iter_on_ref)]

mod cli;
mod config;
mod context;
pub mod lsp;
mod macros;
mod plugin;
mod rule;
mod rule_tester;
mod slice;
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
    sync::{Arc, Mutex, PoisonError},
};

pub use cli::bootstrap_cli;
pub use config::{Args, ArgsBuilder, Config, ConfigBuilder, RuleConfiguration};
use context::PendingFix;
pub use context::QueryMatchContext;
pub use plugin::Plugin;
pub use proc_macros::{builder_args, rule, rule_tests, violation};
use rayon::prelude::*;
use rule::{Captures, InstantiatedRule};
pub use rule::{
    FileRunInfo, MatchBy, NodeOrCaptures, Rule, RuleInstance, RuleInstancePerFile,
    RuleListenerQuery, RuleMeta, ROOT_EXIT,
};
pub use rule_tester::{
    RuleTestExpectedError, RuleTestExpectedErrorBuilder, RuleTestExpectedOutput, RuleTestInvalid,
    RuleTestValid, RuleTester, RuleTests,
};
pub use slice::MutRopeOrSlice;
use tree_sitter::{Query, Tree};
use tree_sitter_grep::{
    get_matches, get_parser, streaming_iterator::StreamingIterator, tree_sitter::QueryMatch,
    Parseable, RopeOrSlice, SupportedLanguage,
};
pub use violation::{ViolationBuilder, ViolationWithContext};

pub extern crate clap;
pub extern crate serde_json;
pub extern crate serde_yaml;
pub extern crate tokio;
pub extern crate tree_sitter_grep;
pub use tree_sitter_grep::{ropey, tree_sitter};

use crate::rule::ResolvedMatchBy;

pub fn run_and_output(config: Config) {
    let violations = run(&config);
    if violations.is_empty() {
        process::exit(0);
    }
    for violation in violations {
        violation.print(&config);
    }
    process::exit(1);
}

const MAX_FIX_ITERATIONS: usize = 10;

#[allow(clippy::too_many_arguments)]
fn run_per_file<'a>(
    aggregated_queries: &'a AggregatedQueries<'a>,
    path: &'a Path,
    config: &'a Config,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: &'a Tree,
    query: &'a Arc<Query>,
    language: SupportedLanguage,
    instantiated_rules: &'a [InstantiatedRule],
) {
    let file_contents = file_contents.into();
    let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>> =
        Default::default();
    get_matches(language.language(), file_contents, query, Some(tree)).for_each(|query_match| {
        run_match(
            query_match,
            aggregated_queries,
            language,
            path,
            file_contents,
            config,
            &mut instantiated_per_file_rules,
            &mut on_found_violations,
            |pending_fixes, instantiated_rule| {
                on_found_pending_fixes(pending_fixes, instantiated_rule)
            },
        );
    });

    for (&instantiated_rule_index, &rule_listener_index) in
        &aggregated_queries.instantiated_rule_root_exit_rule_listener_indices
    {
        run_single_on_query_match_callback(
            path,
            file_contents,
            &instantiated_rules[instantiated_rule_index],
            &mut instantiated_per_file_rules,
            config,
            language,
            rule_listener_index,
            tree.root_node().into(),
            &mut on_found_violations,
            |pending_fixes| {
                on_found_pending_fixes(pending_fixes, &instantiated_rules[instantiated_rule_index])
            },
        );
    }
}

pub fn run(config: &Config) -> Vec<ViolationWithContext> {
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries, None);
    let all_violations: Mutex<HashMap<PathBuf, Vec<ViolationWithContext>>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();
    tree_sitter_grep::run_with_single_per_file_callback(
        tree_sitter_grep_args,
        |dir_entry, language, file_contents, tree, query| {
            run_per_file(
                &aggregated_queries,
                dir_entry.path(),
                config,
                |violations| {
                    all_violations
                        .lock()
                        .unwrap()
                        .entry(dir_entry.path().to_owned())
                        .or_default()
                        .extend(violations)
                },
                |fixes, instantiated_rule| {
                    files_with_fixes.append(
                        dir_entry.path(),
                        file_contents,
                        &instantiated_rule.meta.name,
                        fixes,
                        language,
                    )
                },
                file_contents,
                tree,
                query,
                language,
                &instantiated_rules,
            );
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
                    language,
                },
            )| {
                let mut violations: Vec<ViolationWithContext> = Default::default();
                run_fixing_loop(
                    &mut violations,
                    &mut file_contents,
                    pending_fixes,
                    &aggregated_queries,
                    &path,
                    config,
                    language,
                    &instantiated_rules,
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
fn run_match<'a, 'b>(
    query_match: &'b QueryMatch<'a, 'a>,
    aggregated_queries: &'a AggregatedQueries,
    language: SupportedLanguage,
    path: &'a Path,
    file_contents: impl Into<RopeOrSlice<'a>>,
    config: &'a Config,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    let file_contents = file_contents.into();
    let (instantiated_rule, rule_listener_index, capture_index_if_per_capture) = aggregated_queries
        .get_rule_and_listener_index_and_capture_index(language, query_match.pattern_index);
    match capture_index_if_per_capture {
        Some(capture_index) => {
            query_match
                .captures
                .into_iter()
                .filter(|capture| capture.index == capture_index)
                .for_each(|capture| {
                    run_single_on_query_match_callback(
                        path,
                        file_contents,
                        instantiated_rule,
                        instantiated_per_file_rules,
                        config,
                        language,
                        rule_listener_index,
                        capture.node.into(),
                        &mut on_found_violations,
                        |fixes| on_found_pending_fixes(fixes, instantiated_rule),
                    );
                });
        }
        None => {
            run_single_on_query_match_callback(
                path,
                file_contents,
                instantiated_rule,
                instantiated_per_file_rules,
                config,
                language,
                rule_listener_index,
                Captures::new(
                    query_match,
                    aggregated_queries.get_query_for_language(language),
                )
                .into(),
                on_found_violations,
                |fixes| on_found_pending_fixes(fixes, instantiated_rule),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_single_on_query_match_callback<'a, 'b>(
    path: &'a Path,
    file_contents: impl Into<RopeOrSlice<'a>>,
    instantiated_rule: &'a InstantiatedRule,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    config: &'a Config,
    language: SupportedLanguage,
    rule_listener_index: usize,
    node_or_captures: NodeOrCaptures<'a, 'b>,
    on_found_violations: impl FnOnce(Vec<ViolationWithContext>),
    on_found_pending_fixes: impl FnOnce(Vec<PendingFix>),
) {
    let mut query_match_context =
        QueryMatchContext::new(path, file_contents, instantiated_rule, config, language);
    instantiated_per_file_rules
        .entry(instantiated_rule.meta.name.clone())
        .or_insert_with(|| {
            instantiated_rule
                .rule_instance
                .clone()
                .instantiate_per_file(&FileRunInfo { path })
        })
        .on_query_match(
            rule_listener_index,
            node_or_captures,
            &mut query_match_context,
        );
    if let Some(violations) = query_match_context.violations.take() {
        on_found_violations(violations);
    }
    if let Some(fixes) = query_match_context.into_pending_fixes() {
        assert!(config.fix);
        on_found_pending_fixes(fixes);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_fixing_loop<'a>(
    violations: &mut Vec<ViolationWithContext>,
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    mut pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
    aggregated_queries: &AggregatedQueries,
    path: &Path,
    config: &Config,
    language: SupportedLanguage,
    instantiated_rules: &[InstantiatedRule],
) {
    let mut file_contents = file_contents.into();
    for _ in 0..MAX_FIX_ITERATIONS {
        apply_fixes(&mut file_contents, pending_fixes);
        pending_fixes = Default::default();
        if config.report_fixed_violations {
            *violations = violations
                .iter()
                .filter(|violation| violation.had_fixes)
                .cloned()
                .collect();
        } else {
            violations.clear();
        }
        // TODO: this looks like tree could be passed in and incrementally re-parsed?
        let tree = RopeOrSlice::<'_>::from(&file_contents)
            .parse(&mut get_parser(language.language()), None)
            .unwrap();
        run_per_file(
            aggregated_queries,
            path,
            config,
            |reported_violations| {
                violations.extend(reported_violations);
            },
            |fixes, instantiated_rule| {
                pending_fixes
                    .entry(instantiated_rule.meta.name.clone())
                    .or_default()
                    .extend(fixes);
            },
            &file_contents,
            &tree,
            &aggregated_queries
                .per_language
                .get(&language)
                .unwrap()
                .query,
            language,
            instantiated_rules,
        );
        if pending_fixes.is_empty() {
            break;
        }
    }
}

pub fn run_for_slice<'a>(
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    config: Config,
    language: SupportedLanguage,
) -> Vec<ViolationWithContext> {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if config.fix {
        panic!("Use run_fixing_for_slice()");
    }
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let tree: Cow<'_, Tree> = tree.map_or_else(
        || {
            Cow::Owned(
                file_contents
                    .parse(&mut get_parser(language.language()), None)
                    .unwrap(),
            )
        },
        Cow::Borrowed,
    );
    run_per_file(
        &aggregated_queries,
        path,
        &config,
        |reported_violations| {
            violations.lock().unwrap().extend(reported_violations);
        },
        |_, _| {
            panic!("Expected no fixes");
        },
        file_contents,
        &tree,
        &aggregated_queries
            .per_language
            .get(&language)
            .unwrap()
            .query,
        language,
        &instantiated_rules,
    );
    violations.into_inner().unwrap()
}

pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    config: Config,
    language: SupportedLanguage,
) -> Vec<ViolationWithContext> {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if !config.fix {
        panic!("Use run_for_slice()");
    }
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let tree: Cow<'_, Tree> = tree.map_or_else(
        || {
            Cow::Owned(
                RopeOrSlice::<'_>::from(&file_contents)
                    .parse(&mut get_parser(language.language()), None)
                    .unwrap(),
            )
        },
        Cow::Borrowed,
    );
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let pending_fixes: Mutex<HashMap<RuleName, Vec<PendingFix>>> = Default::default();
    run_per_file(
        &aggregated_queries,
        path,
        &config,
        |reported_violations| {
            violations.lock().unwrap().extend(reported_violations);
        },
        |fixes, instantiated_rule| {
            pending_fixes
                .lock()
                .unwrap()
                .entry(instantiated_rule.meta.name.clone())
                .or_default()
                .extend(fixes);
        },
        &file_contents,
        &tree,
        &aggregated_queries
            .per_language
            .get(&language)
            .unwrap()
            .query,
        language,
        &instantiated_rules,
    );
    let mut violations = violations.into_inner().unwrap();
    let pending_fixes = pending_fixes.into_inner().unwrap();
    if pending_fixes.is_empty() {
        return violations;
    }
    run_fixing_loop(
        &mut violations,
        file_contents,
        pending_fixes,
        &aggregated_queries,
        path,
        &config,
        language,
        &instantiated_rules,
    );
    violations
}

fn get_tree_sitter_grep_args(
    aggregated_queries: &AggregatedQueries,
    language: Option<SupportedLanguage>,
) -> tree_sitter_grep::Args {
    tree_sitter_grep::ArgsBuilder::default()
        .query_per_language(
            aggregated_queries
                .per_language
                .iter()
                .map(|(&language, aggregated_query)| (language, aggregated_query.query.clone()))
                .collect::<HashMap<_, _>>(),
        )
        .maybe_language(language)
        .build()
        .unwrap()
}

fn write_files<'a>(files_to_write: impl Iterator<Item = (&'a Path, &'a [u8])>) {
    for (path, file_contents) in files_to_write {
        fs::write(path, file_contents).unwrap();
    }
}

type RuleName = String;

fn apply_fixes(
    file_contents: &mut MutRopeOrSlice,
    pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
) {
    let non_conflicting_sorted_pending_fixes =
        get_sorted_non_conflicting_pending_fixes(pending_fixes);
    for PendingFix { range, replacement } in non_conflicting_sorted_pending_fixes.into_iter().rev()
    {
        file_contents.splice(range, &replacement);
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
        language: SupportedLanguage,
    ) {
        self.lock()
            .unwrap()
            .entry(path.to_owned())
            .or_insert_with(|| PerFilePendingFixes::new(file_contents.to_owned(), language))
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
    language: SupportedLanguage,
}

impl PerFilePendingFixes {
    fn new(file_contents: Vec<u8>, language: SupportedLanguage) -> Self {
        Self {
            file_contents,
            pending_fixes: Default::default(),
            language,
        }
    }
}

type RuleIndex = usize;
type RuleListenerIndex = usize;
type CaptureIndexIfPerCapture = Option<u32>;

struct AggregatedQueriesPerLanguage {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureIndexIfPerCapture)>,
    query: Arc<Query>,
    #[allow(dead_code)]
    query_text: String,
}

#[derive(Default)]
struct AggregatedQueriesPerLanguageBuilder {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureIndexIfPerCapture)>,
    query_text: String,
}

impl AggregatedQueriesPerLanguageBuilder {
    pub fn build(self, language: SupportedLanguage) -> AggregatedQueriesPerLanguage {
        let Self {
            pattern_index_lookup,
            query_text,
        } = self;
        let query = Arc::new(Query::new(language.language(), &query_text).unwrap());
        assert!(query.pattern_count() == pattern_index_lookup.len());
        AggregatedQueriesPerLanguage {
            pattern_index_lookup,
            query,
            query_text,
        }
    }
}

struct AggregatedQueries<'a> {
    instantiated_rules: &'a [InstantiatedRule],
    per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguage>,
    pub instantiated_rule_root_exit_rule_listener_indices: HashMap<usize, usize>,
}

impl<'a> AggregatedQueries<'a> {
    pub fn new(instantiated_rules: &'a [InstantiatedRule]) -> Self {
        let mut per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguageBuilder> =
            Default::default();
        let mut instantiated_rule_root_exit_rule_listener_indices: HashMap<usize, usize> =
            Default::default();
        for (rule_index, instantiated_rule) in instantiated_rules.into_iter().enumerate() {
            for &language in &instantiated_rule.meta.languages {
                let per_language_builder = per_language.entry(language).or_default();
                for (rule_listener_index, rule_listener_query) in instantiated_rule
                    .rule_instance
                    .listener_queries()
                    .iter()
                    .enumerate()
                    .filter_map(|(rule_listener_index, rule_listener_query)| {
                        if rule_listener_query.query == ROOT_EXIT {
                            instantiated_rule_root_exit_rule_listener_indices
                                .insert(rule_index, rule_listener_index);

                            return None;
                        }

                        Some((
                            rule_listener_index,
                            rule_listener_query.resolve(language.language()),
                        ))
                    })
                {
                    let capture_index_if_per_capture: CaptureIndexIfPerCapture =
                        match &rule_listener_query.match_by {
                            ResolvedMatchBy::PerCapture { capture_index } => Some(*capture_index),
                            _ => None,
                        };

                    for _ in 0..rule_listener_query.query.pattern_count() {
                        per_language_builder.pattern_index_lookup.push((
                            rule_index,
                            rule_listener_index,
                            capture_index_if_per_capture,
                        ));
                    }
                    per_language_builder
                        .query_text
                        .push_str(&rule_listener_query.query_text);
                    per_language_builder.query_text.push_str("\n\n");
                }
            }
        }

        Self {
            instantiated_rules,
            per_language: per_language
                .into_iter()
                .map(|(language, per_language_value)| {
                    (language, per_language_value.build(language))
                })
                .collect(),
            instantiated_rule_root_exit_rule_listener_indices,
        }
    }

    pub fn get_rule_and_listener_index_and_capture_index(
        &self,
        language: SupportedLanguage,
        pattern_index: usize,
    ) -> (&'a InstantiatedRule, usize, CaptureIndexIfPerCapture) {
        let (rule_index, rule_listener_index, capture_index_if_per_capture) = self
            .per_language
            .get(&language)
            .unwrap()
            .pattern_index_lookup[pattern_index];
        let instantiated_rule = &self.instantiated_rules[rule_index];
        (
            instantiated_rule,
            rule_listener_index,
            capture_index_if_per_capture,
        )
    }

    pub fn get_query_for_language(&self, language: SupportedLanguage) -> Arc<Query> {
        self.per_language.get(&language).unwrap().query.clone()
    }
}
