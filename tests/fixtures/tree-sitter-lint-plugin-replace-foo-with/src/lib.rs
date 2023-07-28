use std::sync::Arc;

use tree_sitter_lint::{rule, violation, Plugin, Rule};

pub fn instantiate() -> Plugin {
    Plugin {
        name: "replace-foo-with".to_owned(),
        rules: vec![
            replace_foo_with_bar_rule(),
            replace_foo_with_something_rule(),
        ],
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
        ],
        languages => [Rust]
    }
}

fn replace_foo_with_something_rule() -> Arc<dyn Rule> {
    rule! {
        name => "replace-foo-with-something",
        fixable => true,
        options_type => String,
        state => {
            [per-run]
            replacement: String = options,
        },
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                context.report(
                    violation! {
                        node => node,
                        message => format!(r#"Use '{}' instead of 'foo'"#, self.replacement),
                        fix => |fixer| {
                            fixer.replace_text(node, &self.replacement);
                        }
                    }
                );
            }
        ],
        languages => [Rust]
    }
}
