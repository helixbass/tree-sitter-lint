use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Parser;
use derive_builder::Builder;
use serde::Deserialize;

use crate::{
    rule::{InstantiatedRule, Rule},
    Plugin,
};

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
    pub fn load_config_file_and_into_config(
        self,
        all_plugins: Vec<Plugin>,
        standalone_rules: Vec<Arc<dyn Rule>>,
    ) -> Config {
        let ParsedConfigFile {
            path: config_file_path,
            content: config_file_content,
        } = load_config_file();
        let Args {
            rule,
            fix,
            report_fixed_violations,
        } = self;
        let mut all_rules = standalone_rules;
        all_rules.extend(
            all_plugins
                .iter()
                .flat_map(|plugin| plugin.rules.iter().cloned()),
        );
        Config {
            rule,
            all_rules,
            all_plugins,
            fix,
            report_fixed_violations,
            config_file_path: Some(config_file_path),
            rule_configurations: config_file_content.rules().collect(),
        }
    }
}

#[derive(Builder)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[builder(default)]
    pub rule: Option<String>,

    all_rules: Vec<Arc<dyn Rule>>,

    all_plugins: Vec<Plugin>,

    #[builder(default)]
    pub fix: bool,

    #[builder(default)]
    pub report_fixed_violations: bool,

    #[builder(default)]
    pub config_file_path: Option<PathBuf>,

    pub rule_configurations: Vec<RuleConfiguration>,
}

impl Config {
    fn get_active_rules(&self) -> Vec<Arc<dyn Rule>> {
        self.rule_configurations
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

impl ConfigBuilder {
    pub fn default_rule_configurations(&mut self) -> &mut Self {
        self.rule_configurations = Some(
            self.all_rules
                .as_ref()
                .expect("must call .all_rules() before calling .default_rule_configurations()")
                .into_iter()
                .map(RuleConfiguration::default_for_rule)
                .collect(),
        );
        self
    }
}

#[derive(Clone)]
pub struct ParsedConfigFile {
    pub path: PathBuf,
    pub content: ParsedConfigFileContent,
}

#[derive(Clone, Deserialize)]
pub struct ParsedConfigFileContent {
    pub plugins: HashMap<String, PluginSpecValue>,
    #[serde(rename = "rules")]
    rules_by_name: HashMap<String, RuleConfigurationValue>,
}

impl ParsedConfigFileContent {
    pub fn rules(&self) -> impl Iterator<Item = RuleConfiguration> + '_ {
        self.rules_by_name
            .iter()
            .map(|(rule_name, rule_config)| rule_config.to_rule_configuration(rule_name))
    }
}

#[derive(Clone, Deserialize)]
pub struct PluginSpecValue {
    pub path: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorLevel {
    Error,
    Off,
}

#[derive(Clone, Deserialize)]
pub struct RuleConfigurationValue {
    pub level: ErrorLevel,
}

impl RuleConfigurationValue {
    pub fn to_rule_configuration(&self, rule_name: impl Into<String>) -> RuleConfiguration {
        let rule_name = rule_name.into();
        RuleConfiguration {
            name: rule_name,
            level: self.level,
        }
    }
}

#[derive(Clone)]
pub struct RuleConfiguration {
    pub name: String,
    pub level: ErrorLevel,
}

impl RuleConfiguration {
    pub fn default_for_rule(rule: &Arc<dyn Rule>) -> Self {
        Self {
            name: rule.meta().name,
            level: ErrorLevel::Error,
        }
    }
}

fn load_config_file() -> ParsedConfigFile {
    let config_file_path = find_config_file();
    let config_file_contents =
        fs::read_to_string(&config_file_path).expect("Couldn't read config file contents");
    let parsed = serde_yaml::from_str(&config_file_contents).expect("Couldn't parse config file");

    ParsedConfigFile {
        path: config_file_path,
        content: parsed,
    }
}

const CONFIG_FILENAME: &str = ".tree-sitter-lint.yml";

fn find_config_file() -> PathBuf {
    find_filename_in_ancestor_directory(
        CONFIG_FILENAME,
        env::current_dir().expect("Couldn't get current directory"),
    )
    .expect("Couldn't find config file")
}

// https://codereview.stackexchange.com/a/236771
fn find_filename_in_ancestor_directory(
    filename: impl AsRef<Path>,
    starting_directory: PathBuf,
) -> Option<PathBuf> {
    let filename = filename.as_ref();
    let mut current_path = starting_directory;

    loop {
        current_path.push(filename);

        if current_path.is_file() {
            return Some(current_path);
        }

        if !(current_path.pop() && current_path.pop()) {
            return None;
        }
    }
}
