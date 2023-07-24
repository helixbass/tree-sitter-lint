use std::{path::PathBuf, sync::Arc};

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
    pub fn load_config_file_and_into_config(self, all_rules: Vec<Arc<dyn Rule>>) -> Config {
        let config_file = load_config_file();
        let Args {
            rule,
            fix,
            report_fixed_violations,
        } = self;
        let config = Config {
            rule,
            all_rules,
            fix,
            report_fixed_violations,
            config_file,
        };
        config
    }
}

#[derive(Builder)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[builder(default)]
    pub rule: Option<String>,

    all_rules: Vec<Arc<dyn Rule>>,

    #[builder(default)]
    pub fix: bool,

    #[builder(default)]
    pub report_fixed_violations: bool,

    pub config_file: ParsedConfigFile,
}

impl Config {
    fn get_active_rules(&self) -> Vec<Arc<dyn Rule>> {
        self.config_file
            .content
            .rules
            .iter()
            .filter(|rule_config| rule_config.level != ErrorLevel::Off)
            .map(|rule_config| {
                self.all_rules
                    .iter()
                    .find(|rule| rule.meta().name == rule_config.name)
                    .unwrap_or_else(|| panic!("Unknown rule: '{}'", rule_config.name))
                    .clone()
            })
            .collect()
    }

    fn filter_based_on_rule_argument(
        &self,
        active_rules: Vec<Arc<dyn Rule>>,
    ) -> Vec<Arc<dyn Rule>> {
        match self.rule.as_ref() {
            Some(rule_arg) => {
                let filtered = active_rules
                    .into_iter()
                    .filter(|rule| &rule.meta().name == rule_arg)
                    .collect::<Vec<_>>();
                if !filtered.is_empty() {
                    return filtered;
                }
                self.rule_argument_error();
            }
            None => active_rules,
        }
    }

    fn rule_argument_error(&self) -> ! {
        let rule_arg = self.rule.as_ref().unwrap();
        if self
            .all_rules
            .iter()
            .any(|rule| &rule.meta().name == rule_arg)
        {
            panic!("The '{rule_arg}' rule is configured as inactive");
        } else {
            panic!("Unknown rule: '{rule_arg}'");
        }
    }

    pub fn get_instantiated_rules(&self) -> Vec<InstantiatedRule> {
        let active_rules = self.get_active_rules();
        if active_rules.is_empty() {
            panic!("No configured active rules");
        }
        let active_rules = self.filter_based_on_rule_argument(active_rules);
        let instantiated_rules = active_rules
            .into_iter()
            .map(|rule| InstantiatedRule::new(rule.clone(), self))
            .collect::<Vec<_>>();
        if instantiated_rules.is_empty() {
            panic!("Invalid rule name: {:?}", self.rule.as_ref().unwrap());
        }
        instantiated_rules
    }
}

#[derive(Clone)]
struct ParsedConfigFile {
    pub path: PathBuf,
    pub content: ParsedConfigFileContent,
}

#[derive(Clone)]
struct ParsedConfigFileContent {
    pub rules: Vec<RuleConfiguration>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ErrorLevel {
    Error,
    Off,
}

#[derive(Clone)]
struct RuleConfiguration {
    pub name: String,
    pub level: ErrorLevel,
}

fn load_config_file() -> ParsedConfigFile {}
