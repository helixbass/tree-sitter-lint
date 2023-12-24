use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::prelude::*;

fn main() {
    // tracing_subscriber::fmt::init();

    let (chrome_layer, _guard) = ChromeLayerBuilder::new().include_args(true).build();
    tracing_subscriber::registry().with(chrome_layer).init();

    tree_sitter_lint_local::run_and_output();
}
