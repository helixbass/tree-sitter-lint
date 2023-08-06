use std::sync::Arc;

use tree_sitter_lint::{FromFileRunContextInstanceProviderFactory, Rule};

mod rules;

use rules::{
    no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule,
    require_blazing_keyword_rule,
};

pub fn get_rules<T: FromFileRunContextInstanceProviderFactory>() -> Vec<Arc<dyn Rule<T>>> {
    vec![
        no_default_default_rule(),
        no_lazy_static_rule(),
        prefer_impl_param_rule(),
        require_blazing_keyword_rule(),
    ]
}
