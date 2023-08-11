use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use once_cell::sync::Lazy;
use regex::Regex;
use squalid::regex;
use tracing::{instrument, trace, trace_span};
use tree_sitter_grep::{tree_sitter::Query, RopeOrSlice, SupportedLanguage};

use crate::{
    event_emitter::{self, EventEmitterIndex, EventEmitterName, EventType},
    rule::{InstantiatedRule, ResolvedMatchBy},
    EventEmitter, EventEmitterFactory, EventTypeIndex,
};

type RuleIndex = usize;
type RuleListenerIndex = usize;
type CaptureIndexIfPerCapture = Option<u32>;
type CaptureNameIfPerCapture = Option<String>;
type AllEventEmitterFactoriesIndex = usize;

pub struct AggregatedQueriesPerLanguage {
    pub pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureIndexIfPerCapture)>,
    pub query: Arc<Query>,
    #[allow(dead_code)]
    pub query_text: String,
    pub kind_exit_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    pub kind_enter_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    pub all_active_event_emitter_factories: Vec<Arc<dyn EventEmitterFactory>>,
    pub event_emitter_rule_listener_indices:
        HashMap<(EventEmitterIndex, EventTypeIndex), Vec<(RuleIndex, RuleListenerIndex)>>,
}

impl fmt::Debug for AggregatedQueriesPerLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AggregatedQueriesPerLanguage")
            .field("pattern_index_lookup", &self.pattern_index_lookup)
            .field("query", &self.query)
            .field("query_text", &self.query_text)
            .field(
                "kind_exit_rule_listener_indices",
                &self.kind_exit_rule_listener_indices,
            )
            .field(
                "kind_enter_rule_listener_indices",
                &self.kind_enter_rule_listener_indices,
            )
            // .field(
            //     "all_active_event_emitter_factories",
            //     &self.all_active_event_emitter_factories,
            // )
            .field(
                "event_emitter_rule_listener_indices",
                &self.event_emitter_rule_listener_indices,
            )
            .finish()
    }
}

#[derive(Debug, Default)]
struct AggregatedQueriesPerLanguageBuilder {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureNameIfPerCapture)>,
    query_text: String,
    kind_exit_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    kind_enter_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    all_active_event_emitter_factories: HashMap<EventEmitterName, AllEventEmitterFactoriesIndex>,
    event_emitter_rule_listener_indices: HashMap<
        (AllEventEmitterFactoriesIndex, EventTypeIndex),
        Vec<(RuleIndex, RuleListenerIndex)>,
    >,
}

impl AggregatedQueriesPerLanguageBuilder {
    #[instrument(level = "trace", skip(all_event_emitter_factories))]
    pub fn build(
        self,
        language: SupportedLanguage,
        all_event_emitter_factories: &[Arc<dyn EventEmitterFactory>],
    ) -> AggregatedQueriesPerLanguage {
        let Self {
            pattern_index_lookup,
            mut query_text,
            kind_exit_rule_listener_indices,
            kind_enter_rule_listener_indices,
            event_emitter_rule_listener_indices,
            all_active_event_emitter_factories: all_active_event_emitter_factory_indices,
        } = self;

        if !kind_exit_rule_listener_indices.is_empty()
            || !kind_enter_rule_listener_indices.is_empty()
            || !event_emitter_rule_listener_indices.is_empty()
        {
            query_text.push_str("\n(_) @c");
        }

        let span = trace_span!("parse aggregated query").entered();

        let query = Arc::new(Query::new(language.language(), &query_text).unwrap());

        span.exit();

        let mut all_active_event_emitter_factories: Vec<Arc<dyn EventEmitterFactory>> =
            Default::default();
        let mut all_event_emitter_factories_index_to_event_emitter_index: HashMap<
            AllEventEmitterFactoriesIndex,
            EventEmitterIndex,
        > = Default::default();
        for (_, index) in all_active_event_emitter_factory_indices {
            all_active_event_emitter_factories.push(all_event_emitter_factories[index].clone());
            all_event_emitter_factories_index_to_event_emitter_index
                .insert(index, all_active_event_emitter_factories.len() - 1);
        }

        assert!(
            query.pattern_count()
                == pattern_index_lookup.len()
                    + if kind_exit_rule_listener_indices.is_empty()
                        && kind_enter_rule_listener_indices.is_empty()
                        && event_emitter_rule_listener_indices.is_empty()
                    {
                        0
                    } else {
                        1
                    }
        );
        AggregatedQueriesPerLanguage {
            pattern_index_lookup: {
                let span = trace_span!("resolve capture indexes").entered();

                let pattern_index_lookup = pattern_index_lookup
                    .into_iter()
                    .map(
                        |(rule_index, rule_listener_index, capture_name_if_per_capture)| {
                            (
                                rule_index,
                                rule_listener_index,
                                capture_name_if_per_capture.map(|capture_name| {
                                    query.capture_index_for_name(&capture_name).unwrap()
                                }),
                            )
                        },
                    )
                    .collect::<Vec<_>>();

                span.exit();

                pattern_index_lookup
            },
            query,
            query_text,
            kind_exit_rule_listener_indices,
            kind_enter_rule_listener_indices,
            event_emitter_rule_listener_indices: event_emitter_rule_listener_indices
                .into_iter()
                .map(
                    |((all_event_emitter_factories_index, event_type_index), rule_indices)| {
                        (
                            (
                                all_event_emitter_factories_index_to_event_emitter_index
                                    [&all_event_emitter_factories_index],
                                event_type_index,
                            ),
                            rule_indices,
                        )
                    },
                )
                .collect(),
            all_active_event_emitter_factories,
        }
    }
}

static KIND_ENTER: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^[a-zA-Z_]+$"#).unwrap());
static KIND_EXIT: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^([a-zA-Z_]+):exit$"#).unwrap());

pub struct AggregatedQueries<'a> {
    pub instantiated_rules: &'a [InstantiatedRule],
    pub per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguage>,
}

impl<'a> AggregatedQueries<'a> {
    #[instrument(level = "debug", skip_all)]
    pub fn new(
        instantiated_rules: &'a [InstantiatedRule],
        all_event_emitter_factories: &[Arc<dyn EventEmitterFactory>],
    ) -> Self {
        let mut per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguageBuilder> =
            Default::default();
        #[allow(clippy::type_complexity)]
        let all_event_emitter_factories_by_name: HashMap<
            EventEmitterName,
            (
                AllEventEmitterFactoriesIndex,
                Arc<dyn EventEmitterFactory>,
                HashMap<EventType, EventTypeIndex>,
            ),
        > = all_event_emitter_factories
            .iter()
            .enumerate()
            .map(|(index, event_emitter_factory)| {
                (
                    event_emitter_factory.name(),
                    (
                        index,
                        event_emitter_factory.clone(),
                        event_emitter_factory
                            .event_types()
                            .into_iter()
                            .enumerate()
                            .map(|(index, event_type)| (event_type, index))
                            .collect(),
                    ),
                )
            })
            .collect();

        let span = trace_span!("resolve individual rule listener queries").entered();

        for (rule_index, instantiated_rule) in instantiated_rules.into_iter().enumerate() {
            for &language in &instantiated_rule.meta.languages {
                let per_language_builder = per_language.entry(language).or_default();
                for (rule_listener_index, rule_listener_query) in instantiated_rule
                    .rule_instance
                    .listener_queries()
                    .iter()
                    .enumerate()
                    .filter_map(|(rule_listener_index, rule_listener_query)| {
                        if !rule_listener_query.query.contains('(') {
                            if let Some((event_emitter_name, event_type)) =
                                event_emitter::is_listener(&rule_listener_query.query)
                            {
                                let all_event_emitter_factories_index = *per_language_builder
                                    .all_active_event_emitter_factories
                                    .entry(event_emitter_name.clone())
                                    .or_insert_with(|| {
                                        all_event_emitter_factories_by_name
                                            .get(&event_emitter_name)
                                            .unwrap_or_else(|| panic!("Unknown event emitter"))
                                            .0
                                    });
                                let event_index = *all_event_emitter_factories_by_name
                                    .get(&event_emitter_name)
                                    .unwrap()
                                    .2
                                    .get(&event_type)
                                    .unwrap_or_else(|| panic!("Unknown event type"));
                                per_language_builder
                                    .event_emitter_rule_listener_indices
                                    .entry((all_event_emitter_factories_index, event_index))
                                    .or_default()
                                    .push((rule_index, rule_listener_index));

                                return None;
                            }

                            let mut saw_selector = false;
                            let mut seen_exit_and_enter_kinds: (HashSet<String>, HashSet<&str>) =
                                (Default::default(), Default::default());
                            for selector in
                                regex!(r#"\s*,\s*"#).split(rule_listener_query.query.trim())
                            {
                                if let Some(captures) = KIND_EXIT.captures(selector) {
                                    let kind = &captures[1];
                                    if seen_exit_and_enter_kinds.0.contains(kind) {
                                        panic!("Repeated exit kind");
                                    }
                                    seen_exit_and_enter_kinds.0.insert(kind.to_owned());
                                    per_language_builder
                                        .kind_exit_rule_listener_indices
                                        .entry(kind.to_owned())
                                        .or_default()
                                        .push((rule_index, rule_listener_index));
                                } else if KIND_ENTER.is_match(selector) {
                                    let kind = selector;
                                    if seen_exit_and_enter_kinds.1.contains(kind) {
                                        panic!("Repeated enter kind");
                                    }
                                    seen_exit_and_enter_kinds.1.insert(kind);
                                    per_language_builder
                                        .kind_enter_rule_listener_indices
                                        .entry(kind.to_owned())
                                        .or_default()
                                        .push((rule_index, rule_listener_index));
                                } else {
                                    panic!("Failed to parse non-query selector");
                                }
                                saw_selector = true;
                            }
                            if !saw_selector {
                                panic!("Failed to parse non-query selector");
                            }

                            return None;
                        }

                        Some((
                            rule_listener_index,
                            rule_listener_query.resolve(language.language()),
                        ))
                    })
                {
                    let capture_name_if_per_capture: CaptureNameIfPerCapture =
                        match &rule_listener_query.match_by {
                            ResolvedMatchBy::PerCapture { capture_name } => {
                                Some(capture_name.clone())
                            }
                            _ => None,
                        };

                    for _ in 0..rule_listener_query.query.pattern_count() {
                        per_language_builder.pattern_index_lookup.push((
                            rule_index,
                            rule_listener_index,
                            capture_name_if_per_capture.clone(),
                        ));
                    }
                    per_language_builder
                        .query_text
                        .push_str(&rule_listener_query.query_text);
                    per_language_builder.query_text.push_str("\n\n");
                }
            }
        }

        span.exit();

        Self {
            instantiated_rules,
            per_language: {
                let span = trace_span!("aggregating per-language queries").entered();

                let per_language = per_language
                    .into_iter()
                    .map(|(language, per_language_value)| {
                        (
                            language,
                            per_language_value.build(language, all_event_emitter_factories),
                        )
                    })
                    .collect::<HashMap<_, _>>();

                trace!(?per_language, "aggregated per-language queries");

                span.exit();

                per_language
            },
        }
    }

    pub fn is_wildcard_listener(&self, language: SupportedLanguage, pattern_index: usize) -> bool {
        pattern_index == self.get_wildcard_listener_pattern_index(language)
    }

    pub fn get_wildcard_listener_pattern_index(&self, language: SupportedLanguage) -> usize {
        self.per_language
            .get(&language)
            .unwrap()
            .pattern_index_lookup
            .len()
    }

    pub fn get_kind_exit_rule_and_listener_indices<'b>(
        &'b self,
        language: SupportedLanguage,
        kind: &str,
    ) -> Option<impl Iterator<Item = (&'a InstantiatedRule, RuleListenerIndex)> + 'b> {
        self.per_language[&language]
            .kind_exit_rule_listener_indices
            .get(kind)
            .map(|indices| {
                indices.iter().map(|(rule_index, rule_listener_index)| {
                    (&self.instantiated_rules[*rule_index], *rule_listener_index)
                })
            })
    }

    pub fn get_kind_enter_rule_and_listener_indices<'b>(
        &'b self,
        language: SupportedLanguage,
        kind: &str,
    ) -> Option<impl Iterator<Item = (&'a InstantiatedRule, RuleListenerIndex)> + 'b> {
        self.per_language
            .get(&language)
            .unwrap()
            .kind_enter_rule_listener_indices
            .get(kind)
            .map(|indices| {
                indices.iter().map(|(rule_index, rule_listener_index)| {
                    (&self.instantiated_rules[*rule_index], *rule_listener_index)
                })
            })
    }

    pub fn get_rule_and_listener_index_and_capture_index(
        &self,
        language: SupportedLanguage,
        pattern_index: usize,
    ) -> (
        &'a InstantiatedRule,
        RuleListenerIndex,
        CaptureIndexIfPerCapture,
    ) {
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

    pub fn get_event_emitter_instances<'b>(
        &self,
        language: SupportedLanguage,
        file_contents: impl Into<RopeOrSlice<'b>>,
    ) -> Vec<RefCell<Box<dyn EventEmitter<'b> + 'b>>> {
        let file_contents = file_contents.into();

        self.per_language[&language]
            .all_active_event_emitter_factories
            .iter()
            .map(|event_emitter_factory| RefCell::new(event_emitter_factory.create(file_contents)))
            .collect()
    }

    pub fn get_event_emitter_listeners<'b>(
        &'b self,
        language: SupportedLanguage,
        event_emitter_index: EventEmitterIndex,
        event_type_index: EventTypeIndex,
    ) -> Option<impl Iterator<Item = (&'a InstantiatedRule, RuleListenerIndex)> + 'b> {
        self.per_language[&language]
            .event_emitter_rule_listener_indices
            .get(&(event_emitter_index, event_type_index))
            .map(|indices| {
                indices.iter().map(|(rule_index, rule_listener_index)| {
                    (&self.instantiated_rules[*rule_index], *rule_listener_index)
                })
            })
    }
}
