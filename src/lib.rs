#![allow(clippy::into_iter_on_ref)]

mod aggregated_queries;
mod cli;
mod config;
mod context;
pub mod lsp;
mod macros;
mod node;
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
    sync::{Mutex, PoisonError},
};

use aggregated_queries::AggregatedQueries;
pub use cli::bootstrap_cli;
pub use config::{Args, ArgsBuilder, Config, ConfigBuilder, RuleConfiguration};
pub use context::{FileRunContext, QueryMatchContext, SkipOptions, SkipOptionsBuilder};
use context::{FromFileRunContextInstanceProvider, PendingFix};
pub use node::NodeExt;
pub use plugin::Plugin;
pub use proc_macros::{builder_args, rule, rule_tests, violation};
use rayon::prelude::*;
use rule::{Captures, InstantiatedRule};
pub use rule::{
    MatchBy, NodeOrCaptures, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta,
    ROOT_EXIT,
};
pub use rule_tester::{
    RuleTestExpectedError, RuleTestExpectedErrorBuilder, RuleTestExpectedOutput, RuleTestInvalid,
    RuleTestValid, RuleTester, RuleTests,
};
pub use slice::MutRopeOrSlice;
use tree_sitter::Tree;
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

pub fn run_and_output(
    config: Config,
    get_from_file_run_context_instance_provider: impl Fn() -> Box<dyn FromFileRunContextInstanceProvider>
        + Send
        + Sync,
) {
    let violations = run(&config, get_from_file_run_context_instance_provider);
    if violations.is_empty() {
        process::exit(0);
    }
    for violation in violations {
        violation.print(&config);
    }
    process::exit(1);
}

const MAX_FIX_ITERATIONS: usize = 10;

fn run_per_file<'a, 'b>(
    file_run_context: FileRunContext<'a, 'b>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>> =
        Default::default();
    get_matches(
        file_run_context.language.language(),
        file_run_context.file_contents,
        file_run_context.query,
        Some(file_run_context.tree),
    )
    .for_each(|query_match| {
        run_match(
            file_run_context,
            query_match,
            file_run_context.aggregated_queries,
            &mut instantiated_per_file_rules,
            &mut on_found_violations,
            |pending_fixes, instantiated_rule| {
                on_found_pending_fixes(pending_fixes, instantiated_rule)
            },
        );
    });

    for (&instantiated_rule_index, &rule_listener_index) in &file_run_context
        .aggregated_queries
        .instantiated_rule_root_exit_rule_listener_indices
    {
        run_single_on_query_match_callback(
            file_run_context,
            &file_run_context.instantiated_rules[instantiated_rule_index],
            &mut instantiated_per_file_rules,
            rule_listener_index,
            file_run_context.tree.root_node().into(),
            &mut on_found_violations,
            |pending_fixes| {
                on_found_pending_fixes(
                    pending_fixes,
                    &file_run_context.instantiated_rules[instantiated_rule_index],
                )
            },
        );
    }
}

pub fn run(
    config: &Config,
    get_from_file_run_context_instance_provider: impl Fn() -> Box<dyn FromFileRunContextInstanceProvider>
        + Send
        + Sync,
) -> Vec<ViolationWithContext> {
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries, None);
    let all_violations: Mutex<HashMap<PathBuf, Vec<ViolationWithContext>>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();
    tree_sitter_grep::run_with_single_per_file_callback(
        tree_sitter_grep_args,
        |dir_entry, language, file_contents, tree, query| {
            let from_file_run_context_instance_provider =
                get_from_file_run_context_instance_provider();
            run_per_file(
                FileRunContext::new(
                    dir_entry.path(),
                    file_contents,
                    tree,
                    config,
                    language,
                    &aggregated_queries,
                    query,
                    &instantiated_rules,
                    &*from_file_run_context_instance_provider,
                ),
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
                    &get_from_file_run_context_instance_provider,
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

fn run_match<'a, 'b, 'c>(
    file_run_context: FileRunContext<'a, 'b>,
    query_match: &'c QueryMatch<'a, 'a>,
    aggregated_queries: &'a AggregatedQueries,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    let (instantiated_rule, rule_listener_index, capture_index_if_per_capture) = aggregated_queries
        .get_rule_and_listener_index_and_capture_index(
            file_run_context.language,
            query_match.pattern_index,
        );
    match capture_index_if_per_capture {
        Some(capture_index) => {
            query_match
                .captures
                .into_iter()
                .filter(|capture| capture.index == capture_index)
                .for_each(|capture| {
                    run_single_on_query_match_callback(
                        file_run_context,
                        instantiated_rule,
                        instantiated_per_file_rules,
                        rule_listener_index,
                        capture.node.into(),
                        &mut on_found_violations,
                        |fixes| on_found_pending_fixes(fixes, instantiated_rule),
                    );
                });
        }
        None => {
            run_single_on_query_match_callback(
                file_run_context,
                instantiated_rule,
                instantiated_per_file_rules,
                rule_listener_index,
                Captures::new(
                    query_match,
                    aggregated_queries.get_query_for_language(file_run_context.language),
                )
                .into(),
                on_found_violations,
                |fixes| on_found_pending_fixes(fixes, instantiated_rule),
            );
        }
    }
}

fn run_single_on_query_match_callback<'a, 'b, 'c>(
    file_run_context: FileRunContext<'a, 'b>,
    instantiated_rule: &'a InstantiatedRule,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    rule_listener_index: usize,
    node_or_captures: NodeOrCaptures<'a, 'c>,
    on_found_violations: impl FnOnce(Vec<ViolationWithContext>),
    on_found_pending_fixes: impl FnOnce(Vec<PendingFix>),
) {
    let mut query_match_context = QueryMatchContext::new(file_run_context, instantiated_rule);
    instantiated_per_file_rules
        .entry(instantiated_rule.meta.name.clone())
        .or_insert_with(|| {
            instantiated_rule
                .rule_instance
                .clone()
                .instantiate_per_file(file_run_context)
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
        assert!(file_run_context.config.fix);
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
    get_from_file_run_context_instance_provider: impl Fn()
        -> Box<dyn FromFileRunContextInstanceProvider>,
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
        let from_file_run_context_instance_provider = get_from_file_run_context_instance_provider();
        run_per_file(
            FileRunContext::new(
                path,
                &file_contents,
                &tree,
                config,
                language,
                aggregated_queries,
                &aggregated_queries
                    .per_language
                    .get(&language)
                    .unwrap()
                    .query,
                instantiated_rules,
                &*from_file_run_context_instance_provider,
            ),
            |reported_violations| {
                violations.extend(reported_violations);
            },
            |fixes, instantiated_rule| {
                pending_fixes
                    .entry(instantiated_rule.meta.name.clone())
                    .or_default()
                    .extend(fixes);
            },
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
    get_from_file_run_context_instance_provider: impl Fn() -> Box<dyn FromFileRunContextInstanceProvider>
        + Send
        + Sync,
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
    let from_file_run_context_instance_provider = get_from_file_run_context_instance_provider();
    run_per_file(
        FileRunContext::new(
            path,
            file_contents,
            &tree,
            &config,
            language,
            &aggregated_queries,
            &aggregated_queries
                .per_language
                .get(&language)
                .unwrap()
                .query,
            &instantiated_rules,
            &*from_file_run_context_instance_provider,
        ),
        |reported_violations| {
            violations.lock().unwrap().extend(reported_violations);
        },
        |_, _| {
            panic!("Expected no fixes");
        },
    );
    violations.into_inner().unwrap()
}

pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    config: Config,
    language: SupportedLanguage,
    get_from_file_run_context_instance_provider: impl Fn()
        -> Box<dyn FromFileRunContextInstanceProvider>,
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
    let from_file_run_context_instance_provider = get_from_file_run_context_instance_provider();
    run_per_file(
        FileRunContext::new(
            path,
            &file_contents,
            &tree,
            &config,
            language,
            &aggregated_queries,
            &aggregated_queries
                .per_language
                .get(&language)
                .unwrap()
                .query,
            &instantiated_rules,
            &*from_file_run_context_instance_provider,
        ),
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
        get_from_file_run_context_instance_provider,
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
