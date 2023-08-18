use std::sync::Arc;

use tree_sitter_lint::{
    better_any::tid, rule, tree_sitter_grep::RopeOrSlice, violation, FileRunContext,
    FromFileRunContext, Plugin, Rule,
};

// pub type ProvidedTypes<'a> = ();
pub type ProvidedTypes<'a> = (Foo<'a>,);

#[derive(Clone)]
pub struct Foo<'a> {
    #[allow(dead_code)]
    text: &'a str,
}

impl<'a> FromFileRunContext<'a> for Foo<'a> {
    fn from_file_run_context(file_run_context: FileRunContext<'a, '_>) -> Self {
        Self {
            text: match &file_run_context.file_contents {
                RopeOrSlice::Slice(file_contents) => {
                    std::str::from_utf8(&file_contents[..4]).unwrap()
                }
                _ => unreachable!(),
            },
        }
    }
}

tid! { impl<'a> TidAble<'a> for Foo<'a> }

pub fn instantiate() -> Plugin {
    Plugin {
        name: "replace-foo-with".to_owned(),
        rules: vec![
            replace_foo_with_bar_rule(),
            replace_foo_with_something_rule(),
            starts_with_use_rule(),
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

fn starts_with_use_rule() -> Arc<dyn Rule> {
    rule! {
        name => "starts-with-use",
        listeners => [
            r#"
              (use_declaration) @c
            "# => |node, context| {
                if context.retrieve::<Foo<'a>>().text == "use " {
                    context.report(
                        violation! {
                            node => node,
                            message => r#"Starts with 'use'"#,
                        }
                    );
                }
            }
        ],
        languages => [Rust]
    }
}
