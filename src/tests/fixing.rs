#![cfg(test)]

use std::sync::Arc;

use proc_macros::{rule_crate_internal as rule, violation_crate_internal as violation};

use crate::rule::Rule;

#[macro_export]
macro_rules! assert_fixed_content {
    ($content:literal, $rules:expr, $output:literal $(,)?) => {{
        let mut file_contents = $content.to_owned().into_bytes();
        $crate::run_fixing_for_slice(
            &mut file_contents,
            None,
            "tmp.rs",
            $crate::ConfigBuilder::default()
                .all_standalone_rules($rules)
                .default_rule_configurations()
                .fix(true)
                .build()
                .unwrap(),
            $crate::tree_sitter_grep::SupportedLanguage::Rust,
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

#[test]
fn test_cascading_fixes() {
    assert_fixed_content!(
        r#"
            fn foo() {}
        "#,
        [
            create_identifier_replacing_rule("foo", "bar"),
            create_identifier_replacing_rule("bar", "baz"),
        ],
        r#"
            fn baz() {}
        "#
    );
}

#[test]
fn test_more_than_limit_fix_iterations() {
    assert_fixed_content!(
        r#"
            fn foo() {}
        "#,
        [
            create_identifier_replacing_rule("foo", "foo1"),
            create_identifier_replacing_rule("foo1", "foo2"),
            create_identifier_replacing_rule("foo2", "foo3"),
            create_identifier_replacing_rule("foo3", "foo4"),
            create_identifier_replacing_rule("foo4", "foo5"),
            create_identifier_replacing_rule("foo5", "foo6"),
            create_identifier_replacing_rule("foo6", "foo7"),
            create_identifier_replacing_rule("foo7", "foo8"),
            create_identifier_replacing_rule("foo8", "foo9"),
            create_identifier_replacing_rule("foo9", "foo10"),
            create_identifier_replacing_rule("foo10", "foo11"),
            create_identifier_replacing_rule("foo11", "foo12"),
        ],
        r#"
            fn foo10() {}
        "#
    );
}

#[test]
fn test_conflicting_fixes() {
    assert_fixed_content!(
        r#"
            fn start() {}
        "#,
        [
            create_identifier_replacing_rule("start", "option1"),
            create_identifier_replacing_rule("start", "option2"),
            create_identifier_replacing_rule("option1", "end"),
            create_identifier_replacing_rule("option2", "end"),
        ],
        r#"
            fn end() {}
        "#
    );
}

#[test]
fn test_multiple_nonconflicting_fixes_from_different_rules() {
    assert_fixed_content!(
        r#"
            fn foo() {}
            fn bar() {}
        "#,
        [
            create_identifier_replacing_rule("foo", "baz"),
            create_identifier_replacing_rule("bar", "byz")
        ],
        r#"
            fn baz() {}
            fn byz() {}
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
        ],
        languages => [Rust]
    }
}
