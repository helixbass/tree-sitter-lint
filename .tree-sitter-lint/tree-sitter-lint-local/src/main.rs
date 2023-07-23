use std::sync::Arc;

use tree_sitter_lint::{clap::Parser, Args, Rule};

fn main() {
    tree_sitter_lint::run_and_output(Args::parse().into_config(all_rules()));
}

fn all_rules() -> Vec<Arc<dyn Rule>> {
    local_rules::get_rules()
}
