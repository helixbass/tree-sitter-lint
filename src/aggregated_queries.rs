use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use once_cell::sync::Lazy;
use regex::Regex;
use squalid::regex;
use tracing::{instrument, trace, trace_span};
use tree_sitter_grep::{tree_sitter::Query, SupportedLanguageLanguage};

use crate::rule::{InstantiatedRule, ResolvedMatchBy};

type RuleIndex = usize;
type RuleListenerIndex = usize;
type CaptureIndexIfPerCapture = Option<u32>;
type CaptureNameIfPerCapture = Option<String>;

#[derive(Debug)]
pub struct AggregatedQueriesPerLanguage {
    pub pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureIndexIfPerCapture)>,
    pub query: Arc<Query>,
    #[allow(dead_code)]
    pub query_text: String,
    pub kind_exit_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    pub kind_enter_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
}

#[derive(Debug, Default)]
struct AggregatedQueriesPerLanguageBuilder {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureNameIfPerCapture)>,
    query_text: String,
    kind_exit_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
    kind_enter_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
}

impl AggregatedQueriesPerLanguageBuilder {
    #[instrument(level = "trace")]
    pub fn build(self, language: SupportedLanguageLanguage) -> AggregatedQueriesPerLanguage {
        let Self {
            pattern_index_lookup,
            query_text,
            kind_exit_rule_listener_indices,
            kind_enter_rule_listener_indices,
        } = self;

        let span = trace_span!("parse aggregated query").entered();

        let query = Arc::new(Query::new(language.language(), &query_text).unwrap());

        span.exit();

        assert!(query.pattern_count() == pattern_index_lookup.len() + 1);
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
        }
    }
}

static KIND_ENTER: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^[a-zA-Z_]+$"#).unwrap());
static KIND_EXIT: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^([a-zA-Z_]+):exit$"#).unwrap());

pub struct AggregatedQueries<'a> {
    pub instantiated_rules: &'a [InstantiatedRule],
    pub per_language: HashMap<SupportedLanguageLanguage, AggregatedQueriesPerLanguage>,
}

impl<'a> AggregatedQueries<'a> {
    #[instrument(level = "debug", skip_all)]
    pub fn new(instantiated_rules: &'a [InstantiatedRule]) -> Self {
        let mut per_language: HashMap<SupportedLanguageLanguage, AggregatedQueriesPerLanguageBuilder> =
            Default::default();

        let span = trace_span!("resolve individual rule listener queries").entered();

        for (rule_index, instantiated_rule) in instantiated_rules.into_iter().enumerate() {
            for &language in &instantiated_rule.meta.languages {
                let mut has_seen_successful_parsing_for_this_language: HashMap<RuleListenerIndex, bool> = Default::default();
                for &supported_language_language in language.all_supported_language_languages() {
                    let per_language_builder = per_language.entry(supported_language_language).or_insert_with(|| {
                        AggregatedQueriesPerLanguageBuilder {
                            query_text: "(_) @c\n".to_owned(),
                            ..Default::default()
                        }
                    });
                    for (rule_listener_index, rule_listener_query) in instantiated_rule
                        .rule_instance
                        .listener_queries()
                        .iter()
                        .enumerate()
                        .filter_map(|(rule_listener_index, rule_listener_query)| {
                            if !rule_listener_query.query.contains('(') {
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
                                rule_listener_query.resolve(supported_language_language.language()),
                            ))
                        })
                    {
                        let has_seen_successful_parsing_for_this_language = has_seen_successful_parsing_for_this_language.entry(rule_listener_index).or_default();
                        let Ok(rule_listener_query) = rule_listener_query else {
                            continue;
                        };
                        *has_seen_successful_parsing_for_this_language = true;

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
                if has_seen_successful_parsing_for_this_language.iter().any(|(_, successfully_parsed)| !*successfully_parsed) {
                    panic!("Found a listener query that couldn't be parsed for any language grammar of the supported language");
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
                        (language, per_language_value.build(language))
                    })
                    .collect::<HashMap<_, _>>();

                trace!(?per_language, "aggregated per-language queries");

                span.exit();

                per_language
            },
        }
    }

    pub fn is_wildcard_listener(&self, language: SupportedLanguageLanguage, pattern_index: usize) -> bool {
        pattern_index == self.get_wildcard_listener_pattern_index(language)
    }

    pub fn get_wildcard_listener_pattern_index(&self, _language: SupportedLanguageLanguage) -> usize {
        0
    }

    pub fn get_kind_exit_rule_and_listener_indices<'b>(
        &'b self,
        language: SupportedLanguageLanguage,
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
        language: SupportedLanguageLanguage,
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
        language: SupportedLanguageLanguage,
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
            .pattern_index_lookup[pattern_index - 1];
        let instantiated_rule = &self.instantiated_rules[rule_index];
        (
            instantiated_rule,
            rule_listener_index,
            capture_index_if_per_capture,
        )
    }

    pub fn get_query_for_language(&self, language: SupportedLanguageLanguage) -> Arc<Query> {
        self.per_language.get(&language).unwrap().query.clone()
    }
}
