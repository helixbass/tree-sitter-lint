use crate::{rule, rule::Rule, rule_listener, ViolationBuilder};

pub fn no_default_default_rule() -> Rule {
    rule! {
        name => "no_default_default",
        fixable => true,
        create => |_context| {
            vec![
                rule_listener! {
                    query => r#"(
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
                    capture_name => "c",
                    on_query_match => |node, query_match_context| {
                        query_match_context.report(
                            ViolationBuilder::default()
                                .message(r#"Use '_d()' instead of 'Default::default()'"#)
                                .node(node)
                                .fix(|fixer| {
                                    fixer.replace_text(node, "_d()");
                                })
                                .build()
                                .unwrap(),
                        );
                    }
                }
            ]
        }
    }
}
