#![cfg(test)]

use std::sync::Arc;

use proc_macros::rule;

use crate::{rule::Rule, violation};

#[macro_export]
macro_rules! assert_fixed_content {
    ($content:literal, $rules:expr, $output:literal $(,)?) => {{
        let mut file_contents = $content.to_owned().into_bytes();
        $crate::run_fixing_for_slice(
            &mut file_contents,
            "tmp.rs",
            $crate::ConfigBuilder::default()
                .rules($rules)
                .fix(true)
                .build()
                .unwrap(),
        );
        assert_eq!(
            std::str::from_utf8(&file_contents).unwrap().trim(),
            $output.trim()
        );
    }};
}

#[test]
fn test_single_fix() {
    assert_fixed_content!(
        r#"
            fn foo() {}
        "#,
        [create_identifier_replacing_rule("foo", "bar")],
        r#"
            fn bar() {}
        "#
    );
}

fn create_identifier_replacing_rule(
    name: impl Into<String>,
    replacement: impl Into<String>,
) -> Arc<dyn Rule> {
    rule! {
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
                    violation! {
                        message => format!(r#"Use '{}' instead of '{}'"#, self.replacement, self.name),
                        node => node,
                        fix => |fixer| {
                            fixer.replace_text(node, &self.replacement);
                        },
                    }
                );
            }
        ]
    }
}
