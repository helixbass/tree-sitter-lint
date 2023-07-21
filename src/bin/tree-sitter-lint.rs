use clap::Parser;
use tree_sitter_lint::{run_and_output, Config};

fn main() {
    let config = Config::parse();
    run_and_output(config);
}
