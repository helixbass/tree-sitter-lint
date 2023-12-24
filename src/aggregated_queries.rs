use std::{collections::HashMap, sync::Arc};

use once_cell::sync::Lazy;
use regex::Regex;
use tracing::{instrument, trace, trace_span};
use tree_sitter_grep::{tree_sitter::Query, SupportedLanguage};

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
}

#[derive(Debug, Default)]
struct AggregatedQueriesPerLanguageBuilder {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureNameIfPerCapture)>,
    query_text: String,
    kind_exit_rule_listener_indices: HashMap<String, Vec<(RuleIndex, RuleListenerIndex)>>,
}

impl AggregatedQueriesPerLanguageBuilder {
    #[instrument(level = "trace")]
    pub fn build(self, language: SupportedLanguage) -> AggregatedQueriesPerLanguage {
        let Self {
            pattern_index_lookup,
            mut query_text,
            kind_exit_rule_listener_indices,
        } = self;

        if !kind_exit_rule_listener_indices.is_empty() {
            query_text.push_str(&format!(
                "\n[{}] @c",
                kind_exit_rule_listener_indices
                    .keys()
                    .map(|kind| format!("({kind})"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        let span = trace_span!("parse aggregated query").entered();

        let query = Arc::new(Query::new(language.language(), &query_text).unwrap());

        span.exit();

        assert!(
            query.pattern_count()
                == pattern_index_lookup.len()
                    + if kind_exit_rule_listener_indices.is_empty() {
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
        }
    }
}

static KIND_EXIT: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^([a-zA-Z_]+):exit$"#).unwrap());

pub struct AggregatedQueries<'a> {
    pub instantiated_rules: &'a [InstantiatedRule],
    pub per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguage>,
}

impl<'a> AggregatedQueries<'a> {
    #[instrument(level = "debug", skip_all)]
    pub fn new(instantiated_rules: &'a [InstantiatedRule]) -> Self {
        let mut per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguageBuilder> =
            Default::default();

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
                        if let Some(captures) = KIND_EXIT.captures(&rule_listener_query.query) {
                            per_language_builder
                                .kind_exit_rule_listener_indices
                                .entry(captures[1].to_owned())
                                .or_default()
                                .push((rule_index, rule_listener_index));

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
                        (language, per_language_value.build(language))
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
        self.per_language
            .get(&language)
            .unwrap()
            .kind_exit_rule_listener_indices
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
}
