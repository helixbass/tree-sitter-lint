use clap::Parser;
use tree_sitter_grep::SupportedLanguage;

#[derive(Parser)]
pub struct Config {
    #[arg(short, long)]
    pub language: SupportedLanguage,

    #[arg(long)]
    pub rule: Option<String>,

    #[arg(long)]
    pub fix: bool,
}
