mod args;
mod context;
mod rule;
mod violation;

pub use args::Args;
use rule::{ResolvedRule, Rule, RuleBuilder, RuleListenerBuilder};
use tree_sitter::Query;
use violation::ViolationBuilder;

use crate::context::Context;

pub fn run(args: Args) {
    let language = tree_sitter_rust::language();
    let context = Context::new(language);
    let resolved_rules = get_rules()
        .into_iter()
        .map(|rule| rule.resolve(&context))
        .collect::<Vec<_>>();
    let aggregated_queries = AggregatedQueries::new(&resolved_rules, &context);
}

type RuleIndex = usize;
type RuleListenerIndex = usize;

struct AggregatedQueries {
    pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)>,
    query: Query,
}

impl AggregatedQueries {
    pub fn new(resolved_rules: &[ResolvedRule], context: &Context) -> Self {
        let mut pattern_index_lookup: Vec<(RuleIndex, RuleListenerIndex)> = Default::default();
        let mut aggregated_query_text = String::new();
        for (rule_index, resolved_rule) in resolved_rules.into_iter().enumerate() {
            for (rule_listener_index, rule_listener) in resolved_rule.listeners.iter().enumerate() {
                for _ in 0..rule_listener.query.pattern_count() {
                    pattern_index_lookup.push((rule_index, rule_listener_index));
                }
                aggregated_query_text.push_str(&rule_listener.query_text);
                aggregated_query_text.push_str("\n\n");
            }
        }
        let query = Query::new(context.language, &aggregated_query_text).unwrap();
        assert!(query.pattern_count() == pattern_index_lookup.len());
        Self {
            pattern_index_lookup,
            query,
        }
    }
}

fn get_rules() -> Vec<Rule> {
    vec![no_default_default_rule()]
}

fn no_default_default_rule() -> Rule {
    RuleBuilder::default()
        .name("no_default_default")
        .create(|context| {
            vec![RuleListenerBuilder::default()
                .query(
                    r#"(
                            (call_expression
                              function:
                                (scoped_identifier
                                  path:
                                    (identifier) @first (#eq? @first "Default")
                                  name:
                                    (identifier) @second (#eq? @second "default")
                                )
                            ) @c
                        )"#,
                )
                .capture_name("c")
                .on_query_match(|node| {
                    context.report(
                        ViolationBuilder::default()
                            .message(r#"Use '_d()' instead of 'Default::default()'"#)
                            .node(node)
                            .build()
                            .unwrap(),
                    );
                })
                .build()
                .unwrap()]
        })
        .build()
        .unwrap()
}
