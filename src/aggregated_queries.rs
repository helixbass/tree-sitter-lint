use std::{collections::HashMap, sync::Arc};

use tree_sitter_grep::{tree_sitter::Query, SupportedLanguage};

use crate::{
    rule::{InstantiatedRule, ResolvedMatchBy},
    FromFileRunContextInstanceProvider, ROOT_EXIT,
};

type RuleIndex = usize;
type RuleListenerIndex = usize;
type CaptureIndexIfPerCapture = Option<u32>;

pub struct AggregatedQueriesPerLanguage {
    pub pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex, CaptureIndexIfPerCapture)>,
    pub query: Arc<Query>,
    #[allow(dead_code)]
    pub query_text: String,
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

pub struct AggregatedQueries<
    'a,
    TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider,
> {
    pub instantiated_rules: &'a [InstantiatedRule<TFromFileRunContextInstanceProvider>],
    pub per_language: HashMap<SupportedLanguage, AggregatedQueriesPerLanguage>,
    pub instantiated_rule_root_exit_rule_listener_indices: HashMap<usize, usize>,
}

impl<'a, TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider>
    AggregatedQueries<'a, TFromFileRunContextInstanceProvider>
{
    pub fn new(
        instantiated_rules: &'a [InstantiatedRule<TFromFileRunContextInstanceProvider>],
    ) -> Self {
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
    ) -> (
        &'a InstantiatedRule<TFromFileRunContextInstanceProvider>,
        usize,
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
