use clap::Parser;
use tree_sitter_lint::{run, Args};

fn main() {
    let args = Args::parse();
    run(args);
}
