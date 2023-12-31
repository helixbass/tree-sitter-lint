use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use derive_builder::Builder;
use serde::Deserialize;
use tracing::instrument;

use super::{ErrorLevel, RuleConfiguration};
use crate::{configuration::ConfigurationReference, rule::RuleOptions};

#[derive(Clone)]
pub struct ParsedConfigFile {
    pub path: PathBuf,
    pub content: ParsedConfigFileContent,
}

pub type Plugins = HashMap<String, PluginSpecValue>;

pub type Rules = HashMap<String, RuleConfigurationValue>;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ParsedConfigFileContent {
    pub plugins: Plugins,
    #[serde(default)]
    pub rules: Rules,
    pub tree_sitter_lint_dependency: Option<TreeSitterLintDependencySpec>,
    #[serde(default)]
    pub extends: Vec<ConfigurationReference>,
}

#[derive(Clone, Deserialize)]
pub struct TreeSitterLintDependencySpec {
    pub path: PathBuf,
}

#[derive(Clone, Deserialize)]
pub struct PluginSpecValue {
    pub path: Option<PathBuf>,
}

#[derive(Builder, Clone, Deserialize)]
pub struct RuleConfigurationValue {
    pub level: ErrorLevel,
    #[builder(default, setter(strip_option))]
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

pub fn load_config_file() -> ParsedConfigFile {
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

#[instrument]
pub fn find_config_file() -> PathBuf {
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
