use clap::Parser;

#[derive(Parser)]
pub struct Args {
    #[arg(long)]
    pub rule: Option<String>,

    #[arg(long)]
    pub fix: bool,
}
