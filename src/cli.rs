use std::{
    env,
    process::{self, Command},
};

pub fn bootstrap_cli() {
    let path_to_local_binary =
        "/Users/jrosse/prj/tree-sitter-lint/.tree-sitter-lint/tree-sitter-lint-local/target/release/tree-sitter-lint-local";
    let mut handle = Command::new(path_to_local_binary)
        .args(env::args_os().skip(1))
        .spawn()
        .unwrap();
    process::exit(handle.wait().unwrap().code().unwrap_or(1));
}
