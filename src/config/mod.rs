use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use clap::Parser;
use derive_builder::Builder;
use serde::Deserialize;
use tracing::{instrument, trace_span};

use crate::{
    environment::Environment,
    rule::{InstantiatedRule, Rule, RuleOptions},
    Plugin,
};

mod config_file;
pub use config_file::{
    find_config_file, load_config_file, ParsedConfigFile, Plugins, Rules,
    TreeSitterLintDependencySpec, RuleConfigurationValue, RuleConfigurationValueBuilder
};

use self::config_file::ParsedConfigFileContent;

fn parse_configuration_reference(configuration_reference: &str) -> (&str, &str) {
    let mut chunks = configuration_reference.split('/');
    let plugin_name = chunks.next().unwrap();
    let configuration_name = chunks.next().unwrap();
    assert!(chunks.next().is_none());
    (plugin_name, configuration_name)
}

fn add_rules_from_configuration_reference(
    all_rules_by_name: &mut Rules,
    configuration_reference: &str,
    all_plugins: &[Plugin],
) {
    let (plugin_name, configuration_name) = parse_configuration_reference(configuration_reference);
    let plugin = all_plugins
        .into_iter()
        .find(|plugin| plugin.name == plugin_name)
        .unwrap();
    let configuration = plugin.configs.get(configuration_name).unwrap();
    configuration.extends.iter().for_each(|extend| {
        add_rules_from_configuration_reference(all_rules_by_name, extend, all_plugins);
    });
    all_rules_by_name.extend(
        configuration
            .rules
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
}

fn resolve_rule_configurations(
    config_file_content: &ParsedConfigFileContent,
    all_plugins: &[Plugin],
) -> Vec<RuleConfiguration> {
    let mut all_rules_by_name = Rules::default();
    config_file_content.extends.iter().for_each(|extend| {
        add_rules_from_configuration_reference(&mut all_rules_by_name, extend, all_plugins);
    });
    all_rules_by_name.extend(
        config_file_content
            .rules
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    all_rules_by_name
        .into_iter()
        .map(|(rule_name, rule_config)| rule_config.to_rule_configuration(rule_name))
        .collect()
}

#[derive(Builder, Debug, Default, Parser)]
#[builder(default, setter(into, strip_option))]
pub struct Args {
    pub paths: Vec<PathBuf>,

    #[arg(long)]
    pub rule: Option<String>,

    #[arg(long)]
    pub fix: bool,

    #[arg(long)]
    pub report_fixed_violations: bool,

    #[arg(long)]
    pub force_rebuild: bool,
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
            force_rebuild,
            paths,
        } = self;
        let rule_configurations = resolve_rule_configurations(&config_file_content, &all_plugins);
        Config {
            rule,
            all_standalone_rules,
            all_plugins,
            fix,
            report_fixed_violations,
            paths,
            config_file_path: Some(config_file_path),
            rule_configurations,
            rules_by_plugin_prefixed_name: Default::default(),
            force_rebuild,
            single_fixing_pass: Default::default(),
            environment: Default::default(),
        }
    }
}

pub type PluginIndex = usize;

#[derive(Builder)]
#[builder(setter(strip_option, into), pattern = "owned")]
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

    #[builder(default)]
    pub force_rebuild: bool,

    #[builder(default)]
    pub single_fixing_pass: bool,

    #[builder(default)]
    pub environment: Environment,

    #[builder(default)]
    pub paths: Vec<PathBuf>,
}

impl Config {
    #[allow(clippy::type_complexity)]
    fn get_rules_by_plugin_prefixed_name(
        &self,
    ) -> &HashMap<String, (Arc<dyn Rule>, Option<PluginIndex>)> {
        self.rules_by_plugin_prefixed_name.get_or_init(|| {
            let _span = trace_span!("rules by plugin prefixed name init").entered();

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
                rules_by_plugin_prefixed_name.insert(
                    standalone_rule.meta().name.clone(),
                    (standalone_rule.clone(), None),
                );
            }
            rules_by_plugin_prefixed_name
        })
    }

    #[allow(clippy::type_complexity)]
    #[instrument(level = "trace", skip(self))]
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

    #[allow(clippy::type_complexity)]
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

    #[instrument(level = "debug", skip(self))]
    pub fn get_instantiated_rules(&self) -> Vec<InstantiatedRule> {
        let active_rules_and_associated_plugins_and_options =
            self.get_active_rules_and_associated_plugins_and_options();
        if active_rules_and_associated_plugins_and_options.is_empty() {
            panic!("No configured active rules");
        }
        let active_rules_and_associated_plugins_and_options =
            self.filter_based_on_rule_argument(active_rules_and_associated_plugins_and_options);

        trace_span!("instantiate rules").in_scope(|| {
            active_rules_and_associated_plugins_and_options
                .into_iter()
                .map(|(rule, plugin_index, rule_config)| {
                    InstantiatedRule::new(rule.clone(), plugin_index, rule_config, self)
                })
                .collect()
        })
    }

    pub fn get_plugin_name(&self, plugin_index: PluginIndex) -> &str {
        &self.all_plugins[plugin_index].name
    }
}

impl ConfigBuilder {
    pub fn default_rule_configurations(mut self) -> Self {
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
pub struct RuleConfiguration {
    pub name: String,
    pub level: ErrorLevel,
    pub options: Option<RuleOptions>,
}

impl RuleConfiguration {
    pub fn default_for_rule(rule: &Arc<dyn Rule>) -> Self {
        Self {
            name: rule.meta().name.clone(),
            level: ErrorLevel::Error,
            options: None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorLevel {
    Error,
    Off,
}
