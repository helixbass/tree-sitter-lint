use std::sync::Arc;

use clap::Parser;
use derive_builder::Builder;

use crate::{
    rule::{InstantiatedRule, Rule},
    rules::{no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule},
};

#[derive(Builder, Parser)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[arg(long)]
    #[builder(default)]
    pub rule: Option<String>,

    #[arg(skip)]
    #[builder(default)]
    rules: Option<Vec<Arc<dyn Rule>>>,

    #[arg(long)]
    #[builder(default)]
    pub fix: bool,

    #[arg(long)]
    #[builder(default)]
    pub report_fixed_violations: bool,
}

impl Config {
    fn rules(&self) -> Vec<Arc<dyn Rule>> {
        self.rules.clone().unwrap_or_else(get_dummy_rules)
    }

    pub fn get_instantiated_rules(&self) -> Vec<InstantiatedRule> {
        let instantiated_rules = self
            .rules()
            .into_iter()
            .map(|rule| InstantiatedRule::new(rule, self))
            .filter(|rule| match self.rule.as_ref() {
                Some(rule_arg) => &rule.meta.name == rule_arg,
                None => true,
            })
            .collect::<Vec<_>>();
        if instantiated_rules.is_empty() {
            panic!("Invalid rule name: {:?}", self.rule.as_ref().unwrap());
        }
        instantiated_rules
    }
}

fn get_dummy_rules() -> Vec<Arc<dyn Rule>> {
    vec![
        Arc::new(no_default_default_rule()),
        Arc::new(no_lazy_static_rule()),
        Arc::new(prefer_impl_param_rule()),
    ]
}
