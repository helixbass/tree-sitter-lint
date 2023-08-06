use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use clap::Parser;
use derive_builder::Builder;
use serde::Deserialize;

use crate::{
    rule::{InstantiatedRule, Rule, RuleOptions},
    FromFileRunContextInstanceProvider, Plugin,
};

mod config_file;
pub use config_file::{find_config_file, load_config_file, ParsedConfigFile};

#[derive(Builder, Default, Parser)]
#[builder(default, setter(into, strip_option))]
pub struct Args {
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
    pub fn load_config_file_and_into_config<
        TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider,
    >(
        self,
        all_plugins: Vec<Plugin<TFromFileRunContextInstanceProvider>>,
        all_standalone_rules: Vec<Arc<dyn Rule<TFromFileRunContextInstanceProvider>>>,
    ) -> Config<TFromFileRunContextInstanceProvider> {
        let ParsedConfigFile {
            path: config_file_path,
            content: config_file_content,
        } = load_config_file();
        let Args {
            rule,
            fix,
            report_fixed_violations,
            force_rebuild,
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
            force_rebuild,
        }
    }
}

pub type PluginIndex = usize;

#[derive(Builder)]
#[builder(setter(strip_option, into), pattern = "owned")]
pub struct Config<TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider> {
    #[builder(default)]
    pub rule: Option<String>,

    all_standalone_rules: Vec<Arc<dyn Rule<TFromFileRunContextInstanceProvider>>>,

    #[builder(default)]
    all_plugins: Vec<Plugin<TFromFileRunContextInstanceProvider>>,

    #[builder(default)]
    pub fix: bool,

    #[builder(default)]
    pub report_fixed_violations: bool,

    #[builder(default)]
    pub config_file_path: Option<PathBuf>,

    pub rule_configurations: Vec<RuleConfiguration>,

    #[allow(clippy::type_complexity)]
    #[builder(setter(skip))]
    rules_by_plugin_prefixed_name: OnceLock<
        HashMap<
            String,
            (
                Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
                Option<PluginIndex>,
            ),
        >,
    >,

    #[builder(default)]
    pub force_rebuild: bool,
}

impl<TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider>
    Config<TFromFileRunContextInstanceProvider>
{
    fn get_rules_by_plugin_prefixed_name(
        &self,
    ) -> &HashMap<
        String,
        (
            Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
            Option<PluginIndex>,
        ),
    > {
        self.rules_by_plugin_prefixed_name.get_or_init(|| {
            let mut rules_by_plugin_prefixed_name: HashMap<
                String,
                (
                    Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
                    Option<PluginIndex>,
                ),
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
    ) -> Vec<(
        Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
        Option<PluginIndex>,
        &RuleConfiguration,
    )> {
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
            Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
            Option<PluginIndex>,
            &'a RuleConfiguration,
        )>,
    ) -> Vec<(
        Arc<dyn Rule<TFromFileRunContextInstanceProvider>>,
        Option<PluginIndex>,
        &'a RuleConfiguration,
    )> {
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

    pub fn get_instantiated_rules(
        &self,
    ) -> Vec<InstantiatedRule<TFromFileRunContextInstanceProvider>> {
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

impl<TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider>
    ConfigBuilder<TFromFileRunContextInstanceProvider>
{
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
    pub fn default_for_rule(rule: &Arc<dyn Rule<impl FromFileRunContextInstanceProvider>>) -> Self {
        Self {
            name: rule.meta().name,
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
