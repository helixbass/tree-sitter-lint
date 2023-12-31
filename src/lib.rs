#![allow(clippy::into_iter_on_ref)]

mod aggregated_queries;
mod cli;
mod config;
mod configuration;
mod context;
mod environment;
mod fixing;
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
mod treesitter;
mod violation;
mod visit;

use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use aggregated_queries::AggregatedQueries;
pub use cli::bootstrap_cli;
pub use config::{
    Args, ArgsBuilder, Config, ConfigBuilder, ErrorLevel, RuleConfiguration,
    RuleConfigurationValue, RuleConfigurationValueBuilder,
};
pub use configuration::{Configuration, ConfigurationBuilder};
use context::get_node_parent_cache;
pub use context::{
    get_tokens, CountOptions, CountOptionsBuilder, FileRunContext, FromFileRunContext,
    FromFileRunContextInstanceProvider, FromFileRunContextInstanceProviderFactory,
    FromFileRunContextProvidedTypes, FromFileRunContextProvidedTypesOnceLockStorage,
    NodeParentCache, NodeParentProvider, QueryMatchContext, RunKind, SkipOptions,
    SkipOptionsBuilder, StandaloneNodeParentProvider,
};
use dashmap::DashMap;
use fixing::{run_fixing_loop, AllPendingFixes, PendingFix, PerFilePendingFixes};
pub use fixing::{AccumulatedEdits, Fixer};
use maybe_owned::MaybeOwned;
pub use node::{compare_nodes, NodeExt, NonCommentChildren};
use ouroboros::self_referencing;
pub use plugin::{Plugin, PluginBuilder};
pub use proc_macros::{builder_args, instance_provider_factory, rule, rule_tests, violation};
use rayon::prelude::*;
use rule::{Captures, InstantiatedRule};
pub use rule::{
    MatchBy, NodeOrCaptures, Rule, RuleInstance, RuleInstancePerFile, RuleListenerQuery, RuleMeta,
};
pub use rule_tester::{
    DummyFromFileRunContextInstanceProviderFactory, RuleTestExpectedError,
    RuleTestExpectedErrorBuilder, RuleTestExpectedOutput, RuleTestInvalid, RuleTestInvalidBuilder,
    RuleTestValid, RuleTestValidBuilder, RuleTester, RuleTests,
};
pub use slice::MutRopeOrSlice;
use squalid::EverythingExt;
pub use text::SourceTextProvider;
use tracing::{debug, debug_span, info_span, instrument, trace};
use tree_sitter::Tree;
use tree_sitter_grep::{
    get_matches, get_parser,
    streaming_iterator::StreamingIterator,
    tree_sitter::{Node, QueryMatch},
    Parseable, RopeOrSlice, SupportedLanguage, SupportedLanguageLanguage,
};
pub use treesitter::{
    range_between_end_and_start, range_between_ends, range_between_start_and_end,
    range_between_starts,
};
pub use violation::{ViolationBuilder, ViolationData, ViolationWithContext};
pub use visit::{walk_tree, TreeEnterLeaveVisitor};

pub extern crate better_any;
pub extern crate clap;
pub extern crate const_format;
pub extern crate serde_json;
pub extern crate serde_yaml;
pub extern crate squalid;
pub extern crate tokio;
pub extern crate tree_sitter_grep;
pub use tree_sitter_grep::{ropey, tree_sitter};

pub enum ExitStatus {
    Ok,
    FoundViolations,
}

#[instrument(skip_all)]
pub fn run_and_output(
    config: Config,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) -> ExitStatus {
    let violations = run(&config, from_file_run_context_instance_provider_factory);
    if violations.is_empty() {
        return ExitStatus::Ok;
    }

    let span = info_span!("printing violations", num_violations = violations.len()).entered();

    for violation in violations {
        violation.print(&config);
    }

    span.exit();

    ExitStatus::FoundViolations
}

#[instrument(level = "debug", skip_all)]
pub fn run(
    config: &Config,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
) -> Vec<ViolationWithContext> {
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let tree_sitter_grep_args = get_tree_sitter_grep_args(&aggregated_queries, config, None);
    let all_violations: DashMap<PathBuf, Vec<ViolationWithContext>> = Default::default();
    let files_with_fixes: AllPendingFixes = Default::default();

    let span = info_span!("first pass for all files").entered();

    tree_sitter_grep::run_with_single_per_file_callback(
        tree_sitter_grep_args,
        |dir_entry, supported_language_language, file_contents, tree, query| {
            let from_file_run_context_instance_provider =
                from_file_run_context_instance_provider_factory.create();
            let path = dir_entry.path();
            let node_parent_cache = get_node_parent_cache(tree);
            run_per_file(
                FileRunContext::new(
                    path,
                    file_contents,
                    tree,
                    config,
                    supported_language_language,
                    &aggregated_queries,
                    query,
                    &instantiated_rules,
                    None,
                    &*from_file_run_context_instance_provider,
                    if config.fix {
                        RunKind::CommandLineFixingInitial
                    } else {
                        RunKind::CommandLineNonfixing
                    },
                    &config.environment,
                    &node_parent_cache,
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
                        &instantiated_rule.meta,
                        fixes,
                        supported_language_language,
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
                    RunKind::CommandLineFixingInitial,
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

#[instrument(skip_all, fields(path = ?file_run_context.path, language = ?file_run_context.language()))]
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
        .get_wildcard_listener_pattern_index(file_run_context.supported_language_language);

    let span = debug_span!("loop through query matches").entered();

    get_matches(
        file_run_context.supported_language_language.language(),
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
            file_run_context.supported_language_language,
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
                        .get_query_for_language(file_run_context.supported_language_language),
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
        .get_kind_exit_rule_and_listener_indices(
            file_run_context.supported_language_language,
            exited_node.kind(),
        )
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
        .get_kind_enter_rule_and_listener_indices(
            file_run_context.supported_language_language,
            entered_node.kind(),
        )
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

    let query_match_context = QueryMatchContext::new(file_run_context, instantiated_rule);
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
                &query_match_context,
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

#[self_referencing]
pub struct PerConfigContext {
    instantiated_rules: Vec<InstantiatedRule>,
    #[borrows(instantiated_rules)]
    #[covariant]
    aggregated_queries: AggregatedQueries<'this>,
}

impl fmt::Debug for PerConfigContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PerConfigContext").finish()
    }
}

pub struct SliceRunStatus {
    pub violations: Vec<ViolationWithContext>,
    // pub from_file_run_context_instance_provider: Box<dyn FromFileRunContextInstanceProvider>,
    pub per_config_context: Option<PerConfigContext>,
}

#[instrument(skip_all, fields(path = ?path.as_ref(), ?supported_language_language))]
pub fn run_for_slice<'a>(
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: Option<Tree>,
    path: impl AsRef<Path>,
    config: &Config,
    supported_language_language: SupportedLanguageLanguage,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
    per_config_context: Option<&PerConfigContext>,
) -> SliceRunStatus {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if config.fix {
        panic!("Use run_fixing_for_slice()");
    }
    let per_config_context: MaybeOwned<'_, PerConfigContext> = per_config_context.map_or_else(
        || {
            MaybeOwned::Owned(
                PerConfigContextBuilder {
                    instantiated_rules: config.get_instantiated_rules(),
                    aggregated_queries_builder: |instantiated_rules| {
                        AggregatedQueries::new(instantiated_rules)
                    },
                }
                .build(),
            )
        },
        MaybeOwned::Borrowed,
    );
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    let tree = tree.unwrap_or_else(|| {
        let _span = debug_span!("tree-sitter parse").entered();

        file_contents
            .parse(
                &mut get_parser(supported_language_language.language()),
                None,
            )
            .unwrap()
    });
    let from_file_run_context_instance_provider =
        from_file_run_context_instance_provider_factory.create();
    let node_parent_cache = get_node_parent_cache(&tree);
    run_per_file(
        FileRunContext::new(
            path,
            file_contents,
            &tree,
            config,
            supported_language_language,
            per_config_context.borrow_aggregated_queries(),
            &per_config_context
                .borrow_aggregated_queries()
                .per_language
                .get(&supported_language_language)
                .unwrap()
                .query,
            per_config_context.borrow_instantiated_rules(),
            // TODO: here could wire up "remembered" changed
            // ranges for LSP server use case?
            None,
            &*from_file_run_context_instance_provider,
            RunKind::NonfixingForSlice,
            &config.environment,
            &node_parent_cache,
        ),
        |reported_violations| {
            violations.lock().unwrap().extend(reported_violations);
        },
        |_, _| {
            panic!("Expected no fixes");
        },
    );
    drop(from_file_run_context_instance_provider);
    SliceRunStatus {
        violations: violations.into_inner().unwrap(),
        per_config_context: match per_config_context {
            MaybeOwned::Borrowed(_) => None,
            MaybeOwned::Owned(per_config_context) => Some(per_config_context),
        }, // from_file_run_context_instance_provider,
    }
}

#[instrument(skip_all, fields(path = ?path.as_ref(), ?supported_language_language))]
pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<Tree>,
    path: impl AsRef<Path>,
    config: Config,
    supported_language_language: SupportedLanguageLanguage,
    from_file_run_context_instance_provider_factory: &dyn FromFileRunContextInstanceProviderFactory,
    context: FixingForSliceRunContext,
) -> FixingForSliceRunStatus {
    let file_contents = file_contents.into();
    let path = path.as_ref();
    if !config.fix {
        panic!("Use run_for_slice()");
    }
    let instantiated_rules = config.get_instantiated_rules();
    let aggregated_queries = AggregatedQueries::new(&instantiated_rules);
    let tree = tree.unwrap_or_else(|| {
        let _span = debug_span!("tree-sitter parse").entered();

        RopeOrSlice::<'_>::from(&file_contents)
            .parse(
                &mut get_parser(supported_language_language.language()),
                None,
            )
            .unwrap()
    });
    let violations: Mutex<Vec<ViolationWithContext>> = Default::default();
    #[allow(clippy::type_complexity)]
    let pending_fixes: Mutex<HashMap<RuleName, (Vec<PendingFix>, Arc<RuleMeta>)>> =
        Default::default();
    let from_file_run_context_instance_provider =
        from_file_run_context_instance_provider_factory.create();
    let node_parent_cache = get_node_parent_cache(&tree);
    run_per_file(
        FileRunContext::new(
            path,
            &file_contents,
            &tree,
            &config,
            supported_language_language,
            &aggregated_queries,
            &aggregated_queries
                .per_language
                .get(&supported_language_language)
                .unwrap()
                .query,
            &instantiated_rules,
            // TODO: here could wire up "remembered" changed
            // ranges for LSP server use case?
            None,
            &*from_file_run_context_instance_provider,
            RunKind::FixingForSliceInitial { context: &context },
            &config.environment,
            &node_parent_cache,
        ),
        |reported_violations| {
            violations.lock().unwrap().extend(reported_violations);
        },
        |fixes, instantiated_rule| {
            pending_fixes
                .lock()
                .unwrap()
                .entry(instantiated_rule.meta.name.clone())
                .or_insert_with(|| (Default::default(), instantiated_rule.meta.clone()))
                .0
                .extend(fixes);
        },
    );
    let mut violations = violations.into_inner().unwrap();
    let pending_fixes = pending_fixes.into_inner().unwrap();
    if pending_fixes.is_empty() {
        drop(from_file_run_context_instance_provider);
        return FixingForSliceRunStatus {
            violations,
            instantiated_rules,
            edits: Default::default(),
        };
    }
    drop(from_file_run_context_instance_provider);
    let accumulated_edits = run_fixing_loop(
        &mut violations,
        file_contents,
        pending_fixes,
        &aggregated_queries,
        path,
        &config,
        supported_language_language,
        &instantiated_rules,
        tree,
        from_file_run_context_instance_provider_factory,
        RunKind::FixingForSliceInitial { context: &context },
    );
    FixingForSliceRunStatus {
        violations,
        instantiated_rules,
        edits: Some(accumulated_edits),
    }
}

pub struct FixingForSliceRunStatus {
    violations: Vec<ViolationWithContext>,
    #[allow(dead_code)]
    instantiated_rules: Vec<InstantiatedRule>,
    #[allow(dead_code)]
    edits: Option<AccumulatedEdits>,
}

#[derive(Debug, Default)]
pub struct FixingForSliceRunContext {
    pub last_fixing_run_violations: Option<Vec<ViolationWithContext>>,
    pub edits_since_last_fixing_run: Option<AccumulatedEdits>,
}

fn get_tree_sitter_grep_args(
    aggregated_queries: &AggregatedQueries,
    config: &Config,
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
        .paths(config.paths.clone())
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
