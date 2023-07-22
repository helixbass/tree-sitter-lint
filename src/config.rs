use clap::Parser;
use derive_builder::Builder;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    no_default_default_rule, no_lazy_static_rule, prefer_impl_param_rule,
    rule::{ResolvedRule, Rule},
};

#[derive(Builder, Parser)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[arg(short, long, value_enum)]
    pub language: SupportedLanguage,

    #[arg(long)]
    #[builder(default)]
    pub rule: Option<String>,

    #[arg(long)]
    #[builder(default)]
    pub fix: bool,
}

impl Config {
    pub fn get_resolved_rules(&self) -> Vec<ResolvedRule> {
        let resolved_rules = get_rules()
            .into_iter()
            .filter(|rule| match self.rule.as_ref() {
                Some(rule_arg) => &rule.meta.name == rule_arg,
                None => true,
            })
            .map(|rule| rule.resolve(self))
            .collect::<Vec<_>>();
        if resolved_rules.is_empty() {
            panic!("Invalid rule name: {:?}", self.rule.as_ref().unwrap());
        }
        resolved_rules
    }
}

fn get_rules() -> Vec<Rule> {
    vec![
        no_default_default_rule(),
        no_lazy_static_rule(),
        prefer_impl_param_rule(),
    ]
}
