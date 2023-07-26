use tree_sitter_lint::tokio;

#[tokio::main]
async fn main() {
    tree_sitter_lint_local::run_lsp().await;
}
