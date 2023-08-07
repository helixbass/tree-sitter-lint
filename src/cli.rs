use std::{
    env, fs,
    path::Path,
    process::{self, Command},
};

use clap::Parser;
use inflector::Inflector;
use quote::{format_ident, quote};
use tracing::{debug, debug_span, instrument};

use crate::{
    config::{find_config_file, load_config_file, ParsedConfigFile},
    Args,
};

const PER_PROJECT_DIRECTORY_NAME: &str = ".tree-sitter-lint";

const LOCAL_BINARY_PROJECT_NAME: &str = "tree-sitter-lint-local";

const LOCAL_BINARY_LSP_NAME: &str = "tree-sitter-lint-local-lsp";

#[instrument]
pub fn bootstrap_cli() {
    let config_file_path = find_config_file();
    let project_directory = config_file_path.parent().unwrap();
    let per_project_directory = project_directory.join(PER_PROJECT_DIRECTORY_NAME);
    let local_binary_project_directory = per_project_directory.join(LOCAL_BINARY_PROJECT_NAME);
    let path_to_local_release_binary =
        local_binary_project_directory.join(format!("target/release/{LOCAL_BINARY_PROJECT_NAME}"));
    let command_line_args = env::args_os().collect::<Vec<_>>();

    let span = debug_span!("parse args").entered();

    let args = Args::parse_from(command_line_args.iter().cloned());

    span.exit();

    if should_regenerate_local_binary(&config_file_path, &path_to_local_release_binary, &args) {
        regenerate_local_binary(&local_binary_project_directory, &Path::new("..").join(".."));
    }
    let mut command = Command::new(path_to_local_release_binary);
    command.args(command_line_args.into_iter().skip(1));
    if let Ok(rust_log) = env::var("RUST_LOG") {
        command.env("RUST_LOG", rust_log);
    }
    let mut handle = command.spawn().unwrap();
    process::exit(handle.wait().unwrap().code().unwrap_or(1));
}

#[instrument]
fn should_regenerate_local_binary(
    config_file_path: &Path,
    path_to_local_release_binary: &Path,
    args: &Args,
) -> bool {
    if args.force_rebuild {
        debug!("force rebuild");
        return true;
    }

    let local_release_binary_modified_timestamp = match path_to_local_release_binary
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
    {
        None => return true,
        Some(timestamp) => timestamp,
    };
    let config_file_modified_timestamp = config_file_path
        .metadata()
        .expect("Couldn't read config file metadata")
        .modified()
        .expect("Couldn't get config file modified timestamp");
    config_file_modified_timestamp > local_release_binary_modified_timestamp
}

const LOCAL_RULES_DIR_NAME: &str = "local_rules";

#[instrument]
fn regenerate_local_binary(
    local_binary_project_directory: &Path,
    relative_path_from_local_binary_project_directory_to_project_directory: &Path,
) {
    eprintln!("Config changed, regenerating local binary");
    let parsed_config_file = load_config_file();
    let local_binary_project_src_directory = local_binary_project_directory.join("src");
    let local_binary_project_cargo_toml_path = local_binary_project_directory.join("Cargo.toml");
    if local_binary_project_directory.is_dir() {
        let _ = fs::remove_dir_all(&local_binary_project_src_directory);
        let _ = fs::remove_file(&local_binary_project_cargo_toml_path);
        let _ = fs::remove_file(local_binary_project_directory.join("Cargo.lock"));
    }
    fs::create_dir_all(local_binary_project_directory)
        .expect("Couldn't create local binary project directory");
    let has_local_rules = local_binary_project_directory
        .parent()
        .unwrap()
        .join(LOCAL_RULES_DIR_NAME)
        .is_dir();

    let cargo_toml_contents = get_local_binary_cargo_toml_contents(
        &parsed_config_file,
        has_local_rules,
        relative_path_from_local_binary_project_directory_to_project_directory,
    );
    fs::write(local_binary_project_cargo_toml_path, cargo_toml_contents)
        .expect("Couldn't write local binary project Cargo.toml");

    fs::create_dir(&local_binary_project_src_directory)
        .expect("Couldn't create local binary project `src/` directory");
    let local_binary_project_src_bin_directory = local_binary_project_src_directory.join("bin");
    fs::create_dir(&local_binary_project_src_bin_directory)
        .expect("Couldn't create local binary project `src/bin/` directory");
    let local_binary_crate_name = LOCAL_BINARY_PROJECT_NAME.to_snake_case();
    let src_bin_tree_sitter_lint_local_rs_contents =
        get_src_bin_tree_sitter_lint_local_rs_contents(&local_binary_crate_name);
    fs::write(
        local_binary_project_src_bin_directory.join(format!("{LOCAL_BINARY_PROJECT_NAME}.rs")),
        src_bin_tree_sitter_lint_local_rs_contents,
    )
    .unwrap_or_else(|_| {
        panic!("Couldn't write local binary project src/bin/{LOCAL_BINARY_PROJECT_NAME}.rs",);
    });

    let src_bin_tree_sitter_lint_local_lsp_rs_contents =
        get_src_bin_tree_sitter_lint_local_lsp_rs_contents(&local_binary_crate_name);
    fs::write(
        local_binary_project_src_bin_directory.join(format!("{LOCAL_BINARY_LSP_NAME}.rs")),
        src_bin_tree_sitter_lint_local_lsp_rs_contents,
    )
    .unwrap_or_else(|_| {
        panic!("Couldn't write local binary project src/bin/{LOCAL_BINARY_LSP_NAME}.rs");
    });

    let src_lib_rs_contents = get_src_lib_rs_contents(&parsed_config_file, has_local_rules);
    fs::write(
        local_binary_project_src_directory.join("lib.rs"),
        src_lib_rs_contents,
    )
    .expect("Couldn't write local binary project src/lib.rs");

    let gitignore_contents = get_gitignore_contents();
    fs::write(
        local_binary_project_directory.join(".gitignore"),
        gitignore_contents,
    )
    .expect("Couldn't write local binary project .gitignore");

    release_build_local_binary(local_binary_project_directory);
}

fn release_build_local_binary(local_binary_project_directory: &Path) {
    let output = Command::new("cargo")
        .args(["build", "--release", "--bin", LOCAL_BINARY_PROJECT_NAME])
        .current_dir(local_binary_project_directory)
        .output()
        .expect("Failed to execute cargo release build command");
    if !output.status.success() {
        panic!("Cargo release build of local binary project failed");
    }
}

fn get_local_binary_cargo_toml_contents(
    parsed_config_file: &ParsedConfigFile,
    has_local_rules: bool,
    relative_path_from_local_binary_project_directory_to_project_directory: &Path,
) -> String {
    let mut contents = String::new();
    contents.push_str("[package]\n");
    contents.push_str(&format!("name = \"{}\"\n", LOCAL_BINARY_PROJECT_NAME));
    contents.push_str("version = \"0.1.0\"\n");
    contents.push_str("edition = \"2021\"\n\n");
    contents.push_str("[dependencies]\n");
    contents.push_str("tree-sitter-lint = { path = \"../..\" }\n");
    if has_local_rules {
        contents.push_str(&format!(
            "local_rules = {{ path = \"../{}\" }}\n",
            LOCAL_RULES_DIR_NAME
        ));
    }
    for (plugin, plugin_spec) in &parsed_config_file.content.plugins {
        let path = plugin_spec
            .path
            .as_ref()
            .expect("Currently only handling local path plugin dependencies");
        let path =
            relative_path_from_local_binary_project_directory_to_project_directory.join(path);
        contents.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            get_plugin_crate_name(plugin),
            path.to_str()
                .expect("Couldn't convert plugin path to string")
        ));
    }
    contents.push_str("\n[patch.crates-io]\n");
    contents.push_str("tree-sitter = { git = \"https://github.com/tree-sitter/tree-sitter\", rev = \"c16b90d\" }\n\n");
    contents.push_str("[[bin]]\n");
    contents.push_str(&format!("name = \"{}\"\n\n", LOCAL_BINARY_PROJECT_NAME));
    contents.push_str("[[bin]]\n");
    contents.push_str(&format!("name = \"{}\"\n\n", LOCAL_BINARY_LSP_NAME));
    contents
}

fn get_src_bin_tree_sitter_lint_local_rs_contents(local_binary_crate_name: &str) -> String {
    let local_binary_crate_name = format_ident!("{}", local_binary_crate_name);
    quote! {
        fn main() {
            #local_binary_crate_name::run_and_output();
        }
    }
    .to_string()
}

fn get_src_bin_tree_sitter_lint_local_lsp_rs_contents(local_binary_crate_name: &str) -> String {
    let local_binary_crate_name = format_ident!("{}", local_binary_crate_name);
    quote! {
        use tree_sitter_lint::tokio;

        #[tokio::main]
        async fn main() {
            #local_binary_crate_name::run_lsp().await;
        }
    }
    .to_string()
}

fn get_src_lib_rs_contents(parsed_config_file: &ParsedConfigFile, has_local_rules: bool) -> String {
    let standalone_rules = if has_local_rules {
        quote!(local_rules::get_rules())
    } else {
        quote!(vec![])
    };

    let plugin_crates = parsed_config_file
        .content
        .plugins
        .keys()
        .map(|plugin_name| format_ident!("{}", get_plugin_crate_name(plugin_name).to_snake_case()));

    quote! {
        use std::{path::Path, sync::Arc};

        use tree_sitter_lint::{
            clap::Parser, tree_sitter::Tree, tree_sitter_grep::{RopeOrSlice, SupportedLanguage}, Args, Config, MutRopeOrSlice,
            Plugin, Rule, ViolationWithContext, lsp::{LocalLinter, self},
        };

        pub fn run_and_output() {
            tree_sitter_lint::run_and_output(args_to_config(Args::parse()));
        }

        pub fn run_for_slice<'a>(
            file_contents: impl Into<RopeOrSlice<'a>>,
            tree: Option<&Tree>,
            path: impl AsRef<Path>,
            args: Args,
            language: SupportedLanguage,
        ) -> Vec<ViolationWithContext> {
            tree_sitter_lint::run_for_slice(file_contents, tree, path, args_to_config(args), language)
        }

        pub fn run_fixing_for_slice<'a>(
            file_contents: impl Into<MutRopeOrSlice<'a>>,
            tree: Option<&Tree>,
            path: impl AsRef<Path>,
            args: Args,
            language: SupportedLanguage,
        ) -> Vec<ViolationWithContext> {
            tree_sitter_lint::run_fixing_for_slice(file_contents, tree, path, args_to_config(args), language)
        }

        struct LocalLinterConcrete;

        impl LocalLinter for LocalLinterConcrete {
            fn run_for_slice<'a>(
                &self,
                file_contents: impl Into<RopeOrSlice<'a>>,
                tree: Option<&Tree>,
                path: impl AsRef<Path>,
                args: Args,
                language: SupportedLanguage,
            ) -> Vec<ViolationWithContext> {
                run_for_slice(file_contents, tree, path, args, language)
            }

            fn run_fixing_for_slice<'a>(
                &self,
                file_contents: impl Into<MutRopeOrSlice<'a>>,
                tree: Option<&Tree>,
                path: impl AsRef<Path>,
                args: Args,
                language: SupportedLanguage,
            ) -> Vec<ViolationWithContext> {
                run_fixing_for_slice(file_contents, tree, path, args, language)
            }
        }

        pub async fn run_lsp() {
            lsp::run(LocalLinterConcrete).await;
        }

        fn args_to_config(args: Args) -> Config {
            args.load_config_file_and_into_config(all_plugins(), all_standalone_rules())
        }

        fn all_plugins() -> Vec<Plugin> {
            vec![#(#plugin_crates::instantiate()),*]
        }

        fn all_standalone_rules() -> Vec<Arc<dyn Rule>> {
            #standalone_rules
        }
    }.to_string()
}

fn get_gitignore_contents() -> String {
    let mut contents = String::new();
    contents.push_str("/target\n");
    contents.push_str("/Cargo.lock\n");
    contents
}

fn get_plugin_crate_name(plugin_name: &str) -> String {
    format!("tree-sitter-lint-plugin-{plugin_name}")
}
