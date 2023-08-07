use std::{path::Path, sync::Arc};

use tree_sitter_lint::{
    better_any::TidAble,
    clap::Parser,
    lsp::{self, LocalLinter},
    tree_sitter::Tree,
    tree_sitter_grep::{RopeOrSlice, SupportedLanguage},
    Args, Config, FileRunContext, FromFileRunContext, FromFileRunContextInstanceProvider,
    FromFileRunContextInstanceProviderFactory, FromFileRunContextProvidedTypes,
    FromFileRunContextProvidedTypesOnceLockStorage, MutRopeOrSlice, Plugin, Rule,
    ViolationWithContext,
};

pub fn run_and_output() {
    tracing_subscriber::fmt::init();

    tree_sitter_lint::run_and_output(
        args_to_config(Args::parse()),
        FromFileRunContextInstanceProviderFactoryLocal,
    );
}

pub fn run_for_slice<'a>(
    file_contents: impl Into<RopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    args: Args,
    language: SupportedLanguage,
) -> Vec<ViolationWithContext> {
    tree_sitter_lint::run_for_slice(
        file_contents,
        tree,
        path,
        args_to_config(args),
        language,
        &FromFileRunContextInstanceProviderFactoryLocal,
    )
}

pub fn run_fixing_for_slice<'a>(
    file_contents: impl Into<MutRopeOrSlice<'a>>,
    tree: Option<&Tree>,
    path: impl AsRef<Path>,
    args: Args,
    language: SupportedLanguage,
) -> Vec<ViolationWithContext> {
    tree_sitter_lint::run_fixing_for_slice(
        file_contents,
        tree,
        path,
        args_to_config(args),
        language,
        &FromFileRunContextInstanceProviderFactoryLocal,
    )
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

fn args_to_config<T: FromFileRunContextInstanceProviderFactory>(args: Args) -> Config<T> {
    args.load_config_file_and_into_config(all_plugins(), all_standalone_rules())
}

fn all_plugins<T: FromFileRunContextInstanceProviderFactory>() -> Vec<Plugin<T>> {
    vec![tree_sitter_lint_plugin_replace_foo_with::instantiate()]
}

fn all_standalone_rules<T: FromFileRunContextInstanceProviderFactory>() -> Vec<Arc<dyn Rule<T>>> {
    local_rules::get_rules()
}

struct FromFileRunContextInstanceProviderFactoryLocal;

impl FromFileRunContextInstanceProviderFactory for FromFileRunContextInstanceProviderFactoryLocal {
    type Provider<'a> = FromFileRunContextInstanceProviderLocal<'a>;

    fn create<'a>(&self) -> Self::Provider<'a> {
        FromFileRunContextInstanceProviderLocal {
            tree_sitter_lint_plugin_replace_foo_with_provided_instances:
                tree_sitter_lint_plugin_replace_foo_with::ProvidedTypes::<'a>::once_lock_storage(),
        }
    }
}

struct FromFileRunContextInstanceProviderLocal<'a> {
    tree_sitter_lint_plugin_replace_foo_with_provided_instances:
        <tree_sitter_lint_plugin_replace_foo_with::ProvidedTypes<'a> as FromFileRunContextProvidedTypes::<'a>>::OnceLockStorage,
}

impl<'a> FromFileRunContextInstanceProvider<'a> for FromFileRunContextInstanceProviderLocal<'a> {
    type Parent = FromFileRunContextInstanceProviderFactoryLocal;

    fn get<T: FromFileRunContext<'a> + TidAble<'a>>(
        &self,
        file_run_context: FileRunContext<'a, '_, Self::Parent>,
    ) -> Option<&T> {
        self.tree_sitter_lint_plugin_replace_foo_with_provided_instances
            .get::<T>(file_run_context)
    }
}
