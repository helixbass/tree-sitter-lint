use std::{
    cell::Cell,
    cmp::Ordering,
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

use dashmap::DashMap;
use tracing::{debug, debug_span, instrument};
use tree_sitter_grep::{
    get_parser,
    tree_sitter::{InputEdit, Point, Range, Tree},
    Parseable, RopeOrSlice, SupportedLanguage,
};

use crate::{
    aggregated_queries::AggregatedQueries, event_emitter::EventEmitterIndex,
    rule::InstantiatedRule, run_per_file, Config, FileRunContext,
    FromFileRunContextInstanceProviderFactory, MutRopeOrSlice, RuleMeta, RuleName,
    ViolationWithContext,
};

mod accumulated_edits;
mod fixer;

pub use fixer::{Fixer, PendingFix};

const MAX_FIX_ITERATIONS: usize = 10;

#[allow(clippy::too_many_arguments)]
#[instrument(level = "debug", skip_all, fields(?path))]
pub fn run_fixing_loop<'a>(
    violations: &mut Vec<ViolationWithContext>,
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    mut pending_fixes: HashMap<RuleName, (Vec<PendingFix>, Arc<RuleMeta>)>,
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

        if config.single_fixing_pass {
            return;
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
        let event_emitters =
            aggregated_queries.get_event_emitter_instances(language, &file_contents);
        let current_event_emitter_index: Cell<Option<EventEmitterIndex>> = Default::default();
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
                &event_emitters,
                &current_event_emitter_index,
            ),
            |reported_violations| {
                violations.extend(reported_violations);
            },
            |fixes, instantiated_rule| {
                pending_fixes
                    .entry(instantiated_rule.meta.name.clone())
                    .or_insert_with(|| (Default::default(), instantiated_rule.meta.clone()))
                    .0
                    .extend(fixes);
            },
        );
        if pending_fixes.is_empty() {
            debug!("no fixes reported, exiting fixing loop");
            break;
        }
    }
}

#[instrument(level = "debug", skip_all)]
pub fn apply_fixes(
    file_contents: &mut MutRopeOrSlice,
    pending_fixes: HashMap<RuleName, (Vec<PendingFix>, Arc<RuleMeta>)>,
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
    let mut prev_start = None;
    let mut prev_end = None;
    for pending_fix in sorted_pending_fixes {
        if let Some(prev_end) = prev_end {
            if pending_fix.range.start_byte < prev_end {
                return true;
            }
        }
        if let Some(prev_start) = prev_start {
            if pending_fix.range.start_byte <= prev_start {
                return true;
            }
        }
        prev_end = Some(pending_fix.range.end_byte);
        prev_start = Some(pending_fix.range.start_byte);
    }
    false
}

fn get_non_overlapping_subset(sorted_pending_fixes: &[PendingFix]) -> Vec<PendingFix> {
    let mut prev_start = None;
    let mut prev_end = None;
    sorted_pending_fixes
        .into_iter()
        .filter(|pending_fix| {
            if let Some(prev_end) = prev_end {
                if pending_fix.range.start_byte < prev_end {
                    return false;
                }
            }
            if let Some(prev_start) = prev_start {
                if pending_fix.range.start_byte <= prev_start {
                    return false;
                }
            }
            prev_end = Some(pending_fix.range.end_byte);
            prev_start = Some(pending_fix.range.start_byte);
            true
        })
        .cloned()
        .collect()
}

fn get_sorted_non_conflicting_pending_fixes(
    pending_fixes: HashMap<RuleName, (Vec<PendingFix>, Arc<RuleMeta>)>,
) -> Vec<PendingFix> {
    pending_fixes.into_iter().fold(
        Default::default(),
        |accumulated_fixes, (rule_name, (mut pending_fixes_for_rule, rule_meta))| {
            pending_fixes_for_rule.sort_by(compare_pending_fixes);
            if has_overlapping_ranges(&pending_fixes_for_rule) {
                if !rule_meta.allow_self_conflicting_fixes {
                    panic!("Rule {:?} tried to apply self-conflicting fixes: {pending_fixes_for_rule:#?}", rule_name);
                }
                pending_fixes_for_rule = get_non_overlapping_subset(&pending_fixes_for_rule);
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
pub struct AllPendingFixes(DashMap<PathBuf, PerFilePendingFixes>);

impl AllPendingFixes {
    pub fn append(
        &self,
        path: &Path,
        file_contents: &[u8],
        rule_meta: &Arc<RuleMeta>,
        fixes: Vec<PendingFix>,
        language: SupportedLanguage,
        tree: Tree,
    ) {
        self.entry(path.to_owned())
            .or_insert_with(|| PerFilePendingFixes::new(file_contents.to_owned(), language, tree))
            .pending_fixes
            .entry(rule_meta.name.to_owned())
            .or_insert_with(|| (Default::default(), rule_meta.clone()))
            .0
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

pub struct PerFilePendingFixes {
    pub file_contents: Vec<u8>,
    pub pending_fixes: HashMap<RuleName, (Vec<PendingFix>, Arc<RuleMeta>)>,
    pub language: SupportedLanguage,
    pub tree: Tree,
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
