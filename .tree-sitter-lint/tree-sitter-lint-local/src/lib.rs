use std::{path::Path, sync::Arc};

use tree_sitter_lint::{
    clap::Parser, tree_sitter::Tree, tree_sitter_grep::RopeOrSlice, Args, Config, MutRopeOrSlice,
    Plugin, Rule, ViolationWithContext, lsp::{LocalLinter, self},
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

pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    args: Args,
) -> Vec<ViolationWithContext> {
    tree_sitter_lint::run_fixing_for_slice(file_contents, tree, path, args_to_config(args))
}

struct LocalLinterConcrete;

impl LocalLinter for LocalLinterConcrete {
    fn run_for_slice<'a>(
        &self,
        file_contents: impl Into<RopeOrSlice<'a>>,
        tree: Option<&Tree>,
        path: impl AsRef<Path>,
        args: Args,
    ) -> Vec<ViolationWithContext> {
        run_for_slice(file_contents, tree, path, args)
    }

    fn run_fixing_for_slice<'a>(
        &self,
        file_contents: impl Into<MutRopeOrSlice<'a>>,
        tree: Option<&Tree>,
        path: impl AsRef<Path>,
        args: Args,
    ) -> Vec<ViolationWithContext> {
        run_fixing_for_slice(file_contents, tree, path, args)
    }
}

pub async fn run_lsp() {
    lsp::run(LocalLinterConcrete).await;
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
