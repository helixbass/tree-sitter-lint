#![allow(clippy::into_iter_on_ref)]

mod aggregated_queries;
mod cli;
mod config;
mod context;
pub mod event_emitter;
pub mod lsp;
mod macros;
mod node;
mod plugin;
mod rule;
mod rule_tester;
mod slice;
#[cfg(test)]
mod tests;
mod text;
mod violation;

use std::{
    cmp::Ordering,
    collections::HashMap,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process,
    sync::Mutex,
};

use aggregated_queries::AggregatedQueries;
pub use cli::bootstrap_cli;
pub use config::{Args, ArgsBuilder, Config, ConfigBuilder, RuleConfiguration};
use context::PendingFix;
pub use context::{
    FileRunContext, FromFileRunContext, FromFileRunContextInstanceProvider,
    FromFileRunContextInstanceProviderFactory, FromFileRunContextProvidedTypes,
    FromFileRunContextProvidedTypesOnceLockStorage, QueryMatchContext, SkipOptions,
    SkipOptionsBuilder,
};
use dashmap::DashMap;
pub use event_emitter::{Event, EventEmitter, EventEmitterFactory};
pub use node::NodeExt;
pub use plugin::Plugin;
pub use proc_macros::{builder_args, rule, rule_tests, violation};
use rayon::prelude::*;
use rule::{Captures, InstantiatedRule};
pub use rule::{
    MatchBy, NodeOrCaptures, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta,
};
pub use rule_tester::{
    RuleTestExpectedError, RuleTestExpectedErrorBuilder, RuleTestExpectedOutput, RuleTestInvalid,
    RuleTestValid, RuleTester, RuleTests,
};
pub use slice::MutRopeOrSlice;
use squalid::EverythingExt;
pub use text::SourceTextProvider;
use tracing::{debug, debug_span, info_span, instrument, trace};
use tree_sitter::Tree;
use tree_sitter_grep::{
    get_matches, get_parser,
    streaming_iterator::StreamingIterator,
    tree_sitter::{InputEdit, Node, Point, QueryMatch, Range},
    Parseable, RopeOrSlice, SupportedLanguage,
};
pub use violation::{ViolationBuilder, ViolationWithContext};

pub extern crate better_any;
pub extern crate clap;
pub extern crate serde_json;
pub extern crate serde_yaml;
pub extern crate tokio;
pub extern crate tree_sitter_grep;
pub use tree_sitter_grep::{ropey, tree_sitter};

#[instrument(skip_all)]
pub fn run_and_output(
    config: Config,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) {
    let violations = run(&config, from_file_run_context_instance_provider_factory);
    if violations.is_empty() {
        process::exit(0);
    }

    let span = info_span!("printing violations", num_violations = violations.len()).entered();

    for violation in violations {
        violation.print(&config);
    }

    span.exit();

    process::exit(1);
}

const MAX_FIX_ITERATIONS: usize = 10;

#[instrument(level = "debug", skip_all)]
pub fn run(
    config: &Config,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) -> Vec<ViolationWithContext> {
    let instantiated_rules = config.get_instantiated_rules();
    let all_event_emitter_factories = config.get_all_event_emitter_factories();
    let aggregated_queries =
        AggregatedQueries::new(&instantiated_rules, &all_event_emitter_factories);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries, None);
    let all_violations: DashMap<PathBuf, Vec<ViolationWithContext>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();

    let span = info_span!("first pass for all files").entered();

    tree_sitter_grep::run_with_single_per_file_callback(
        tree_sitter_grep_args,
        |dir_entry, language, file_contents, tree, query| {
            let from_file_run_context_instance_provider =
                from_file_run_context_instance_provider_factory.create();
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
                    None,
                    &*from_file_run_context_instance_provider,
                ),
                |violations| {
                    all_violations
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
                        tree.clone(),
                    )
                },
            );
        },
    )
    .unwrap();

    span.exit();

    if !config.fix {
        let violations = all_violations
            .into_iter()
            .flat_map(|(_, value)| value)
            .collect::<Vec<_>>();

        debug!(
            num_violations = violations.len(),
            "non-fixing mode, returning after initial pass"
        );

        return violations;
    }
    let files_with_fixes = files_with_fixes.into_inner();
    if files_with_fixes.is_empty() {
        let violations = all_violations
            .into_iter()
            .flat_map(|(_, value)| value)
            .collect::<Vec<_>>();

        debug!(
            num_violations = violations.len(),
            "fixing mode, returning after initial pass"
        );

        return violations;
    }

    let span = info_span!("running fixing loop for all files").entered();

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
                    tree,
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
                    tree,
                    from_file_run_context_instance_provider_factory,
                );
                (path, (file_contents, violations))
            },
        )
        .collect();

    span.exit();

    write_files(
        aggregated_results_from_files_with_fixes
            .iter()
            .map(|(path, (file_contents, _))| (&**path, &**file_contents)),
    );
    for (path, (_, violations)) in aggregated_results_from_files_with_fixes {
        all_violations.insert(path, violations);
    }
    all_violations
        .into_iter()
        .flat_map(|(_, value)| value)
        .collect()
}

#[instrument(skip_all, fields(path = ?file_run_context.path, language = ?file_run_context.language))]
fn run_per_file<'a, 'b>(
    file_run_context: FileRunContext<'a, 'b>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    let mut instantiated_per_file_rules: HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>> =
        Default::default();
    let mut node_stack: Vec<Node<'a>> = Vec::with_capacity(16);
    let mut saw_match = false;
    let wildcard_listener_pattern_index = file_run_context
        .aggregated_queries
        .get_wildcard_listener_pattern_index(file_run_context.language);

    let span = debug_span!("loop through query matches").entered();

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
            &mut instantiated_per_file_rules,
            &mut node_stack,
            wildcard_listener_pattern_index,
            &mut on_found_violations,
            |pending_fixes, instantiated_rule| {
                on_found_pending_fixes(pending_fixes, instantiated_rule)
            },
        );
        saw_match = true;
    });

    span.exit();

    // for Rust at least I'm not seeing it fire a match
    // for the root grammar node against an empty source
    // file?
    if !saw_match {
        run_exit_node_listeners(
            file_run_context.tree.root_node(),
            file_run_context,
            &mut instantiated_per_file_rules,
            &mut on_found_violations,
            &mut on_found_pending_fixes,
        );
        return;
    }
    while let Some(node) = node_stack.pop() {
        run_exit_node_listeners(
            node,
            file_run_context,
            &mut instantiated_per_file_rules,
            &mut on_found_violations,
            &mut on_found_pending_fixes,
        );
    }
}

#[instrument(level = "debug", skip_all, fields(?query_match))]
fn run_match<'a, 'b, 'c>(
    file_run_context: FileRunContext<'a, 'b>,
    query_match: &'c QueryMatch<'a, 'a>,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    node_stack: &mut Vec<Node<'a>>,
    wildcard_listener_pattern_index: usize,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    if query_match.pattern_index == wildcard_listener_pattern_index {
        assert!(query_match.captures.len() == 1);
        let node = query_match.captures[0].node;

        while !node_stack.is_empty() && node.end_byte() > node_stack.last().unwrap().end_byte() {
            run_exit_node_listeners(
                node_stack.pop().unwrap(),
                file_run_context,
                instantiated_per_file_rules,
                &mut on_found_violations,
                &mut on_found_pending_fixes,
            );
        }
        node_stack.push(node);
        run_enter_node_listeners(
            node,
            file_run_context,
            instantiated_per_file_rules,
            &mut on_found_violations,
            &mut on_found_pending_fixes,
        );
        return;
    }

    let (instantiated_rule, rule_listener_index, capture_index_if_per_capture) = file_run_context
        .aggregated_queries
        .get_rule_and_listener_index_and_capture_index(
            file_run_context.language,
            query_match.pattern_index,
        );

    trace!(
        rule_name = instantiated_rule.meta.name,
        rule_listener_index,
        capture_index_if_per_capture,
        "found query match"
    );

    match capture_index_if_per_capture {
        Some(capture_index) => {
            query_match
                .captures
                .into_iter()
                .filter(|capture| capture.index == capture_index)
                .for_each(|capture| {
                    let node = capture.node;

                    // I don't know if this "guarantees" that exit listeners
                    // will be fired at the "expected" time wrt query listeners
                    // (what about the "match-oriented" case below?)?
                    // May be better to just "be symmetrical" and use "enter"
                    // listeners to correspond to "exit" listeners (vs using
                    // query listeners as the "enter" listeners)?
                    while !node_stack.is_empty()
                        && node.end_byte() > node_stack.last().unwrap().end_byte()
                    {
                        run_exit_node_listeners(
                            node_stack.pop().unwrap(),
                            file_run_context,
                            instantiated_per_file_rules,
                            &mut on_found_violations,
                            &mut on_found_pending_fixes,
                        );
                    }

                    run_single_on_query_match_callback(
                        file_run_context,
                        instantiated_rule,
                        instantiated_per_file_rules,
                        rule_listener_index,
                        node.into(),
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
                    file_run_context
                        .aggregated_queries
                        .get_query_for_language(file_run_context.language),
                )
                .into(),
                on_found_violations,
                |fixes| on_found_pending_fixes(fixes, instantiated_rule),
            );
        }
    }
}

#[instrument(level = "trace", skip_all)]
fn run_exit_node_listeners<'a, 'b>(
    exited_node: Node<'a>,
    file_run_context: FileRunContext<'a, 'b>,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    if let Some(kind_exit_rule_listener_indices) = file_run_context
        .aggregated_queries
        .get_kind_exit_rule_and_listener_indices(file_run_context.language, exited_node.kind())
    {
        kind_exit_rule_listener_indices.for_each(|(instantiated_rule, rule_listener_index)| {
            run_single_on_query_match_callback(
                file_run_context,
                instantiated_rule,
                instantiated_per_file_rules,
                rule_listener_index,
                exited_node.into(),
                &mut on_found_violations,
                |fixes| on_found_pending_fixes(fixes, instantiated_rule),
            );
        });
    }
}

#[instrument(level = "trace", skip_all)]
fn run_enter_node_listeners<'a, 'b>(
    entered_node: Node<'a>,
    file_run_context: FileRunContext<'a, 'b>,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    mut on_found_violations: impl FnMut(Vec<ViolationWithContext>),
    mut on_found_pending_fixes: impl FnMut(Vec<PendingFix>, &InstantiatedRule),
) {
    if let Some(kind_enter_rule_listener_indices) = file_run_context
        .aggregated_queries
        .get_kind_enter_rule_and_listener_indices(file_run_context.language, entered_node.kind())
    {
        kind_enter_rule_listener_indices.for_each(|(instantiated_rule, rule_listener_index)| {
            run_single_on_query_match_callback(
                file_run_context,
                instantiated_rule,
                instantiated_per_file_rules,
                rule_listener_index,
                entered_node.into(),
                &mut on_found_violations,
                |fixes| on_found_pending_fixes(fixes, instantiated_rule),
            );
        });
    }
}

#[instrument(level = "debug", skip_all)]
fn run_single_on_query_match_callback<'a, 'b, 'c>(
    file_run_context: FileRunContext<'a, 'b>,
    instantiated_rule: &'a InstantiatedRule,
    instantiated_per_file_rules: &mut HashMap<RuleName, Box<dyn RuleInstancePerFile<'a> + 'a>>,
    rule_listener_index: usize,
    node_or_captures: NodeOrCaptures<'a, 'c>,
    on_found_violations: impl FnOnce(Vec<ViolationWithContext>),
    on_found_pending_fixes: impl FnOnce(Vec<PendingFix>),
) {
    trace!("running single on query match callback");

    let mut query_match_context = QueryMatchContext::new(file_run_context, instantiated_rule);
    instantiated_per_file_rules
        .entry(instantiated_rule.meta.name.clone())
        .or_insert_with(|| {
            let _span = debug_span!(
                "instantiate rule per file",
                name = instantiated_rule.meta.name
            )
            .entered();

            instantiated_rule
                .rule_instance
                .clone()
                .instantiate_per_file(file_run_context)
        })
        .thrush(|rule_instance_per_file| {
            let _span = debug_span!(
                "run rule listener callback",
                name = instantiated_rule.meta.name,
                rule_listener_index
            )
            .entered();

            rule_instance_per_file.on_query_match(
                rule_listener_index,
                node_or_captures,
                &mut query_match_context,
            );
        });

    if let Some(violations) = query_match_context.violations.take() {
        on_found_violations(violations);
    }

    if let Some(fixes) = query_match_context.into_pending_fixes() {
        assert!(file_run_context.config.fix);
        on_found_pending_fixes(fixes);
    }
}

#[allow(clippy::too_many_arguments)]
#[instrument(level = "debug", skip_all, fields(?path))]
fn run_fixing_loop<'a>(
    violations: &mut Vec<ViolationWithContext>,
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    mut pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
    aggregated_queries: &AggregatedQueries,
    path: &Path,
    config: &Config,
    language: SupportedLanguage,
    instantiated_rules: &[InstantiatedRule],
    tree: Tree,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) {
    let mut file_contents = file_contents.into();
    let mut old_tree = tree;
    for _ in 0..MAX_FIX_ITERATIONS {
        let _span = debug_span!("single fixing loop pass").entered();

        let input_edits = apply_fixes(&mut file_contents, pending_fixes);
        for input_edit in &input_edits {
            old_tree.edit(input_edit);
        }
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

        let parse_span = debug_span!("tree-sitter parse").entered();

        let tree = RopeOrSlice::<'_>::from(&file_contents)
            .parse(&mut get_parser(language.language()), Some(&old_tree))
            .unwrap();
        let changed_ranges = old_tree.changed_ranges(&tree).collect::<Vec<_>>();

        parse_span.exit();

        let from_file_run_context_instance_provider =
            from_file_run_context_instance_provider_factory.create();
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
                Some(&changed_ranges),
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
            debug!("no fixes reported, exiting fixing loop");
            break;
        }
    }
}

// pub struct RunForSliceStatus {
//     pub violations: Vec<ViolationWithContext>,
//     pub from_file_run_context_instance_provider: Box<dyn FromFileRunContextInstanceProvider>,
// }

#[instrument(skip_all, fields(path = ?path.as_ref(), ?language))]
pub fn run_for_slice<'a>(
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: Option<Tree>,
    path: impl AsRef<Path>,
    config: Config,
    language: SupportedLanguage,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) -> Vec<ViolationWithContext> {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if config.fix {
        panic!("Use run_fixing_for_slice()");
    }
    let instantiated_rules = config.get_instantiated_rules();
    let all_event_emitter_factories = config.get_all_event_emitter_factories();
    let aggregated_queries =
        AggregatedQueries::new(&instantiated_rules, &all_event_emitter_factories);
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let tree = tree.unwrap_or_else(|| {
        let _span = debug_span!("tree-sitter parse").entered();

        file_contents
            .parse(&mut get_parser(language.language()), None)
            .unwrap()
    });
    let from_file_run_context_instance_provider =
        from_file_run_context_instance_provider_factory.create();
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
            // TODO: here could wire up "remembered" changed
            // ranges for LSP server use case?
            None,
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
    // RunForSliceStatus {
    //     violations: violations.into_inner().unwrap(),
    //     from_file_run_context_instance_provider,
    // }
}

#[instrument(skip_all, fields(path = ?path.as_ref(), ?language))]
pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<Tree>,
    path: impl AsRef<Path>,
    config: Config,
    language: SupportedLanguage,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) -> Vec<ViolationWithContext> {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if !config.fix {
        panic!("Use run_for_slice()");
    }
    let instantiated_rules = config.get_instantiated_rules();
    let all_event_emitter_factories = config.get_all_event_emitter_factories();
    let aggregated_queries =
        AggregatedQueries::new(&instantiated_rules, &all_event_emitter_factories);
    let tree = tree.unwrap_or_else(|| {
        let _span = debug_span!("tree-sitter parse").entered();

        RopeOrSlice::<'_>::from(&file_contents)
            .parse(&mut get_parser(language.language()), None)
            .unwrap()
    });
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let pending_fixes: Mutex<HashMap<RuleName, Vec<PendingFix>>> = Default::default();
    let from_file_run_context_instance_provider =
        from_file_run_context_instance_provider_factory.create();
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
            // TODO: here could wire up "remembered" changed
            // ranges for LSP server use case?
            None,
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
    drop(from_file_run_context_instance_provider);
    run_fixing_loop(
        &mut violations,
        file_contents,
        pending_fixes,
        &aggregated_queries,
        path,
        &config,
        language,
        &instantiated_rules,
        tree,
        from_file_run_context_instance_provider_factory,
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

#[instrument(level = "debug", skip_all)]
fn write_files<'a>(files_to_write: impl Iterator<Item = (&'a Path, &'a [u8])>) {
    for (path, file_contents) in files_to_write {
        let _span = debug_span!("write file", ?path).entered();

        fs::write(path, file_contents).unwrap();
    }
}

type RuleName = String;

#[instrument(level = "debug", skip_all)]
fn apply_fixes(
    file_contents: &mut MutRopeOrSlice,
    pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
) -> Vec<InputEdit> {
    let non_conflicting_sorted_pending_fixes =
        get_sorted_non_conflicting_pending_fixes(pending_fixes);
    non_conflicting_sorted_pending_fixes
        .into_iter()
        .rev()
        .map(|PendingFix { range, replacement }| {
            file_contents.splice(range.start_byte..range.end_byte, &replacement);
            get_input_edit(range, &replacement)
        })
        .collect()
}

fn get_updated_end_point(range: Range, replacement: &str) -> Point {
    let mut end_point: Point = range.end_point;
    for ch in replacement.chars() {
        if ch == '\n' {
            end_point.row += 1;
            end_point.column = 0;
        } else {
            end_point.column += 1;
        }
    }
    end_point
}

fn get_input_edit(range: Range, replacement: &str) -> InputEdit {
    InputEdit {
        start_byte: range.start_byte,
        old_end_byte: range.end_byte,
        new_end_byte: range.start_byte + replacement.len(),
        start_position: range.start_point,
        old_end_position: range.end_point,
        new_end_position: get_updated_end_point(range, replacement),
    }
}

fn compare_pending_fixes(a: &PendingFix, b: &PendingFix) -> Ordering {
    if a.range.start_byte < b.range.start_byte {
        return Ordering::Less;
    }
    if a.range.start_byte > b.range.start_byte {
        return Ordering::Greater;
    }
    if a.range.end_byte < b.range.end_byte {
        return Ordering::Less;
    }
    if a.range.end_byte > b.range.end_byte {
        return Ordering::Greater;
    }
    Ordering::Equal
}

fn has_overlapping_ranges(sorted_pending_fixes: &[PendingFix]) -> bool {
    let mut prev_end = None;
    for pending_fix in sorted_pending_fixes {
        if let Some(prev_end) = prev_end {
            if pending_fix.range.start_byte < prev_end {
                return true;
            }
        }
        prev_end = Some(pending_fix.range.end_byte);
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
struct AllPendingFixes(DashMap<PathBuf, PerFilePendingFixes>);

impl AllPendingFixes {
    pub fn append(
        &self,
        path: &Path,
        file_contents: &[u8],
        rule_name: &str,
        fixes: Vec<PendingFix>,
        language: SupportedLanguage,
        tree: Tree,
    ) {
        self.entry(path.to_owned())
            .or_insert_with(|| PerFilePendingFixes::new(file_contents.to_owned(), language, tree))
            .pending_fixes
            .entry(rule_name.to_owned())
            .or_default()
            .extend(fixes);
    }

    pub fn into_inner(self) -> HashMap<PathBuf, PerFilePendingFixes> {
        self.0.into_iter().collect()
    }
}

impl Deref for AllPendingFixes {
    type Target = DashMap<PathBuf, PerFilePendingFixes>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct PerFilePendingFixes {
    file_contents: Vec<u8>,
    pending_fixes: HashMap<RuleName, Vec<PendingFix>>,
    language: SupportedLanguage,
    tree: Tree,
}

impl PerFilePendingFixes {
    fn new(file_contents: Vec<u8>, language: SupportedLanguage, tree: Tree) -> Self {
        Self {
            file_contents,
            pending_fixes: Default::default(),
            language,
            tree,
        }
    }
}
