use clap::Parser;
use tree_sitter_lint::{run, Config};

fn main() {
    let config = Config::parse();
    run(config);
}
