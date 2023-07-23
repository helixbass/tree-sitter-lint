use std::sync::Arc;

use clap::Parser;
use derive_builder::Builder;

use crate::rule::{InstantiatedRule, Rule};

#[derive(Parser)]
pub struct Args {
    #[arg(long)]
    pub rule: Option<String>,

    #[arg(long)]
    pub fix: bool,

    #[arg(long)]
    pub report_fixed_violations: bool,
}

impl Args {
    pub fn into_config(self, rules: Vec<Arc<dyn Rule>>) -> Config {
        let Args {
            rule,
            fix,
            report_fixed_violations,
        } = self;
        Config {
            rule,
            rules,
            fix,
            report_fixed_violations,
        }
    }
}

#[derive(Builder)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[builder(default)]
    pub rule: Option<String>,

    rules: Vec<Arc<dyn Rule>>,

    #[builder(default)]
    pub fix: bool,

    #[builder(default)]
    pub report_fixed_violations: bool,
}

impl Config {
    fn rules(&self) -> &[Arc<dyn Rule>] {
        &self.rules
    }

    pub fn get_instantiated_rules(&self) -> Vec<InstantiatedRule> {
        let instantiated_rules = self
            .rules()
            .into_iter()
            .map(|rule| InstantiatedRule::new(rule.clone(), self))
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
