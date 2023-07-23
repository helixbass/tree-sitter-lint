#![cfg(test)]

use std::sync::Arc;

use proc_macros::rule;
use tree_sitter::Node;

use crate::{
    context::QueryMatchContext, rule::Rule, run_fixing_for_slice, ConfigBuilder, ViolationBuilder,
};

#[test]
fn test_single_fix() {
    let mut file_contents = r#"
        fn foo() {}
    "#
    .to_owned()
    .into_bytes();
    run_fixing_for_slice(
        &mut file_contents,
        "tmp.rs",
        ConfigBuilder::default()
            .rules([create_identifier_replacing_rule("foo", "bar")])
            .fix(true)
            .build()
            .unwrap(),
    );
    assert_eq!(
        std::str::from_utf8(&file_contents).unwrap().trim(),
        r#"
            fn bar() {}
        "#
        .trim()
    );
}

fn create_identifier_replacing_rule(
    name: impl Into<String>,
    replacement: impl Into<String>,
) -> Arc<dyn Rule> {
    Arc::new(rule! {
        name => format!("replace_{}_with_{}", self.name, self.replacement),
        fixable => true,
        state => {
            [rule-static]
            name: String = name.into(),
            replacement: String = replacement.into(),
        },
        listeners => [
            format!(r#"(
              (identifier) @c (#eq? @c "{}")
            )"#, self.name) => |node, context| {
                context.report(
                    ViolationBuilder::default()
                        .message(
                            format!(r#"Use '{}' instead of '{}'"#, self.replacement, self.name)
                        )
                        .node(node)
                        .fix(|fixer| {
                            fixer.replace_text(node, &self.replacement);
                        })
                        .build()
                        .unwrap(),
                );
            }
        ]
    })
}
