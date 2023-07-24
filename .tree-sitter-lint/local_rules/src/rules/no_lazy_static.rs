use std::sync::Arc;

use tree_sitter_lint::{rule, violation, Rule};

pub fn no_lazy_static_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no-lazy-static",
        listeners => [
            r#"(
              (macro_invocation
                 macro: (identifier) @c (#eq? @c "lazy_static")
              )
            )"# => |node, context| {
                context.report(
                    violation! {
                        message => r#"Prefer 'OnceCell::*::Lazy' to 'lazy_static!()'"#,
                        node => node,
                    }
                );
            }
        ]
    }
}
