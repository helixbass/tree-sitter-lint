use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use clap::Parser;
use derive_builder::Builder;
use serde::Deserialize;

use crate::{
    rule::{InstantiatedRule, Rule, RuleOptions},
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
        all_standalone_rules: Vec<Arc<dyn Rule>>,
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
        Config {
            rule,
            all_standalone_rules,
            all_plugins,
            fix,
            report_fixed_violations,
            config_file_path: Some(config_file_path),
            rule_configurations: config_file_content.rules().collect(),
            rules_by_plugin_prefixed_name: Default::default(),
        }
    }
}

pub type PluginIndex = usize;

#[derive(Builder)]
#[builder(setter(strip_option, into))]
pub struct Config {
    #[builder(default)]
    pub rule: Option<String>,

    all_standalone_rules: Vec<Arc<dyn Rule>>,

    #[builder(default)]
    all_plugins: Vec<Plugin>,

    #[builder(default)]
    pub fix: bool,

    #[builder(default)]
    pub report_fixed_violations: bool,

    #[builder(default)]
    pub config_file_path: Option<PathBuf>,

    pub rule_configurations: Vec<RuleConfiguration>,

    #[allow(clippy::type_complexity)]
    #[builder(setter(skip))]
    rules_by_plugin_prefixed_name: OnceLock<HashMap<String, (Arc<dyn Rule>, Option<PluginIndex>)>>,
}

impl Config {
    fn get_rules_by_plugin_prefixed_name(
        &self,
    ) -> &HashMap<String, (Arc<dyn Rule>, Option<PluginIndex>)> {
        self.rules_by_plugin_prefixed_name.get_or_init(|| {
            let mut rules_by_plugin_prefixed_name: HashMap<
                String,
                (Arc<dyn Rule>, Option<PluginIndex>),
            > = self
                .all_plugins
                .iter()
                .enumerate()
                .flat_map(|(plugin_index, plugin)| {
                    plugin.rules.iter().map(move |rule| {
                        (
                            format!("{}/{}", plugin.name, rule.meta().name),
                            (rule.clone(), Some(plugin_index)),
                        )
                    })
                })
                .collect();
            for standalone_rule in &self.all_standalone_rules {
                rules_by_plugin_prefixed_name
                    .insert(standalone_rule.meta().name, (standalone_rule.clone(), None));
            }
            rules_by_plugin_prefixed_name
        })
    }

    fn get_active_rules_and_associated_plugins_and_options(
        &self,
    ) -> Vec<(Arc<dyn Rule>, Option<PluginIndex>, &RuleConfiguration)> {
        let rules_by_plugin_prefixed_name = self.get_rules_by_plugin_prefixed_name();
        self.rule_configurations
            .iter()
            .filter(|rule_config| rule_config.level != ErrorLevel::Off)
            .map(|rule_config| {
                let (rule, plugin_index) = rules_by_plugin_prefixed_name
                    .get(&rule_config.name)
                    .unwrap_or_else(|| panic!("Unknown rule: '{}'", rule_config.name))
                    .clone();
                (rule, plugin_index, rule_config)
            })
            .collect()
    }

    fn filter_based_on_rule_argument<'a>(
        &self,
        active_rules_and_associated_plugins_and_options: Vec<(
            Arc<dyn Rule>,
            Option<PluginIndex>,
            &'a RuleConfiguration,
        )>,
    ) -> Vec<(Arc<dyn Rule>, Option<PluginIndex>, &'a RuleConfiguration)> {
        match self.rule.as_ref() {
            Some(rule_arg) => {
                let filtered = active_rules_and_associated_plugins_and_options
                    .into_iter()
                    .filter(|(rule, _, _)| &rule.meta().name == rule_arg)
                    .collect::<Vec<_>>();
                if !filtered.is_empty() {
                    return filtered;
                }
                self.rule_argument_error();
            }
            None => active_rules_and_associated_plugins_and_options,
        }
    }

    fn rule_argument_error(&self) -> ! {
        let rule_arg = self.rule.as_ref().unwrap();
        if self
            .get_rules_by_plugin_prefixed_name()
            .contains_key(rule_arg)
        {
            panic!("The '{rule_arg}' rule is configured as inactive");
        } else {
            panic!("Unknown rule: '{rule_arg}'");
        }
    }

    pub fn get_instantiated_rules(&self) -> Vec<InstantiatedRule> {
        let active_rules_and_associated_plugins_and_options =
            self.get_active_rules_and_associated_plugins_and_options();
        if active_rules_and_associated_plugins_and_options.is_empty() {
            panic!("No configured active rules");
        }
        let active_rules_and_associated_plugins_and_options =
            self.filter_based_on_rule_argument(active_rules_and_associated_plugins_and_options);
        active_rules_and_associated_plugins_and_options
            .into_iter()
            .map(|(rule, plugin_index, rule_config)| {
                InstantiatedRule::new(rule.clone(), plugin_index, rule_config, self)
            })
            .collect()
    }

    pub fn get_plugin_name(&self, plugin_index: PluginIndex) -> &str {
        &self.all_plugins[plugin_index].name
    }
}

impl ConfigBuilder {
    pub fn default_rule_configurations(&mut self) -> &mut Self {
        self.rule_configurations = Some(
            self.all_standalone_rules
                .as_ref()
                .expect("must call .all_standalone_rules() before calling .default_rule_configurations()")
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
    pub options: Option<RuleOptions>,
}

impl RuleConfigurationValue {
    pub fn to_rule_configuration(&self, rule_name: impl Into<String>) -> RuleConfiguration {
        let rule_name = rule_name.into();
        RuleConfiguration {
            name: rule_name,
            level: self.level,
            options: self.options.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RuleConfiguration {
    pub name: String,
    pub level: ErrorLevel,
    pub options: Option<RuleOptions>,
}

impl RuleConfiguration {
    pub fn default_for_rule(rule: &Arc<dyn Rule>) -> Self {
        Self {
            name: rule.meta().name,
            level: ErrorLevel::Error,
            options: None,
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
