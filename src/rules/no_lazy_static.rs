use crate::{
    rule::{Rule, RuleBuilder, RuleListenerBuilder},
    ViolationBuilder,
};

pub fn no_lazy_static_rule() -> Rule {
    RuleBuilder::default()
        .name("no_lazy_static")
        .create(|_context| {
            vec![RuleListenerBuilder::default()
                .query(
                    r#"(
                      (macro_invocation
                         macro: (identifier) @c (#eq? @c "lazy_static")
                      )
                    )"#,
                )
                .on_query_match(|node, query_match_context| {
                    query_match_context.report(
                        ViolationBuilder::default()
                            .message(r#"Prefer 'OnceCell::*::Lazy' to 'lazy_static!()'"#)
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
