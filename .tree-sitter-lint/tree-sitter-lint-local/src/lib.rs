use std::{path::Path, sync::Arc};

use tree_sitter_lint::{
    clap::Parser, tree_sitter::Tree, tree_sitter_grep::RopeOrSlice, Args, Config, Plugin, Rule,
    ViolationWithContext,
};

pub fn run_and_output() {
    tree_sitter_lint::run_and_output(args_to_config(Args::parse()));
}

pub fn run_for_slice<'a>(
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    args: Args,
) -> Vec<ViolationWithContext> {
    tree_sitter_lint::run_for_slice(file_contents, tree, path, args_to_config(args))
}

fn args_to_config(args: Args) -> Config {
    args.load_config_file_and_into_config(all_plugins(), all_standalone_rules())
}

fn all_plugins() -> Vec<Plugin> {
    vec![tree_sitter_lint_plugin_replace_foo_with::instantiate()]
}

fn all_standalone_rules() -> Vec<Arc<dyn Rule>> {
    local_rules::get_rules()
}
