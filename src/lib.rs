mod args;
mod context;
mod rule;
mod violation;

pub use args::Args;
use rule::{Rule, RuleBuilder, RuleListenerBuilder};
use violation::ViolationBuilder;

use crate::context::Context;

pub fn run(args: Args) {
    let language = tree_sitter_rust::language();
    let context = Context::new(language);
    let resolved_rules = get_rules()
        .into_iter()
        .map(|rule| rule.resolve(&context))
        .collect::<Vec<_>>();
    unimplemented!()
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
