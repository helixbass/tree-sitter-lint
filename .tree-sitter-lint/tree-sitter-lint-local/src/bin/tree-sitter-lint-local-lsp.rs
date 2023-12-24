use std :: env ; use tracing_chrome :: ChromeLayerBuilder ; use tracing_subscriber :: { prelude :: * , EnvFilter } ; use tree_sitter_lint :: { tokio , squalid :: NonEmpty } ; # [tokio :: main] async fn main () { if env :: var ("TRACE_CHROME") . ok () . is_non_empty () { let (chrome_layer , _guard) = ChromeLayerBuilder :: new () . include_args (true) . file ("/Users/jrosse/prj/hello-world/trace.json") . build () ; tracing_subscriber :: registry () . with (chrome_layer) . init () ; } else if let Some (tracing_log_file_path) = env :: var ("TRACING_LOG_PATH") . ok () . non_empty () { let out_log = std :: fs :: OpenOptions :: new () . write (true) . append (true) . create (true) . open (tracing_log_file_path) . unwrap () ; tracing_subscriber :: fmt () . with_env_filter (EnvFilter :: from_default_env ()) . with_writer (out_log) . init () ; } tree_sitter_lint_local :: run_lsp () . await ; }