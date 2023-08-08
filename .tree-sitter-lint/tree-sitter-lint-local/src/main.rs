use std::sync::Arc;

use tree_sitter_lint::{clap::Parser, Args, Plugin, Rule};

fn main() {
    tree_sitter_lint::run_and_output(
        Args::parse().load_config_file_and_into_config(all_plugins(), all_standalone_rules()),
    );
}

fn all_plugins() -> Vec<Plugin> {
    vec![tree_sitter_lint_plugin_replace_foo_with::instantiate()]
}

fn all_standalone_rules() -> Vec<Arc<dyn Rule>> {
    local_rules::get_rules()
}
