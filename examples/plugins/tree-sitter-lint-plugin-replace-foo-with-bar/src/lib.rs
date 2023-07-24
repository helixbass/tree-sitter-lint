use std::sync::Arc;

use tree_sitter_lint::{rule, violation, Plugin, Rule};

pub fn instantiate() -> Plugin {
    Plugin {
        rules: vec![replace_foo_with_bar_rule()],
    }
}

fn replace_foo_with_bar_rule() -> Arc<dyn Rule> {
    rule! {
        name => "replace-foo-with-bar",
        fixable => true,
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                context.report(
                    violation! {
                        node => node,
                        message => r#"Use 'bar' instead of 'foo'"#,
                        fix => |fixer| {
                            fixer.replace_text(node, "bar");
                        }
                    }
                );
            }
        ]
    }
}
