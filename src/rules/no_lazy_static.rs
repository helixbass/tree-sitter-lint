use std::sync::Arc;

use proc_macros::rule;

use crate::{rule::Rule, violation};

pub fn no_lazy_static_rule() -> Arc<dyn Rule> {
    rule! {
        name => "no_lazy_static",
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
