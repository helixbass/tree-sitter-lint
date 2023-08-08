use std::sync::Arc;

use tree_sitter_lint::Rule;

mod rules;

use rules::{
    no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule,
    require_blazing_keyword_rule,
};

pub fn get_rules() -> Vec<Arc<dyn Rule>> {
    vec![
        no_default_default_rule(),
        no_lazy_static_rule(),
        prefer_impl_param_rule(),
        require_blazing_keyword_rule(),
    ]
}
