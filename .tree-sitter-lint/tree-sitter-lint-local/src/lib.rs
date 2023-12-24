use std :: { any :: TypeId , path :: Path , sync :: Arc } ; use tree_sitter_lint :: { better_any :: Tid , clap :: Parser , tree_sitter :: Tree , tree_sitter_grep :: { RopeOrSlice , SupportedLanguage } , Args , Config , FileRunContext , FromFileRunContextInstanceProvider , FromFileRunContextInstanceProviderFactory , FromFileRunContextProvidedTypes , FromFileRunContextProvidedTypesOnceLockStorage , MutRopeOrSlice , Plugin , Rule , ViolationWithContext , lsp :: { LocalLinter , self } , FixingForSliceRunStatus , } ; pub fn run_and_output () { tree_sitter_lint :: run_and_output (args_to_config (Args :: parse ()) , & FromFileRunContextInstanceProviderFactoryLocal ,) ; } pub fn run_for_slice < 'a > (file_contents : impl Into < RopeOrSlice < 'a >> , tree : Option < Tree > , path : impl AsRef < Path > , args : Args , language : SupportedLanguage ,) -> Vec < ViolationWithContext > { tree_sitter_lint :: run_for_slice (file_contents , tree , path , args_to_config (args) , language , & FromFileRunContextInstanceProviderFactoryLocal ,) . 0 } pub fn run_fixing_for_slice < 'a > (file_contents : impl Into < MutRopeOrSlice < 'a >> , tree : Option < Tree > , path : impl AsRef < Path > , args : Args , language : SupportedLanguage ,) -> FixingForSliceRunStatus { tree_sitter_lint :: run_fixing_for_slice (file_contents , tree , path , args_to_config (args) , language , & FromFileRunContextInstanceProviderFactoryLocal ,) } struct LocalLinterConcrete ; impl LocalLinter for LocalLinterConcrete { fn run_for_slice < 'a > (& self , file_contents : impl Into < RopeOrSlice < 'a >> , tree : Option < Tree > , path : impl AsRef < Path > , args : Args , language : SupportedLanguage ,) -> Vec < ViolationWithContext > { run_for_slice (file_contents , tree , path , args , language) } fn run_fixing_for_slice < 'a > (& self , file_contents : impl Into < MutRopeOrSlice < 'a >> , tree : Option < Tree > , path : impl AsRef < Path > , args : Args , language : SupportedLanguage ,) -> FixingForSliceRunStatus { run_fixing_for_slice (file_contents , tree , path , args , language) } } pub async fn run_lsp () { lsp :: run (LocalLinterConcrete) . await ; } fn args_to_config (args : Args) -> Config { args . load_config_file_and_into_config (all_plugins () , all_standalone_rules ()) } fn all_plugins () -> Vec < Plugin > { vec ! [tree_sitter_lint_plugin_replace_foo_with :: instantiate ()] } fn all_standalone_rules () -> Vec < Arc < dyn Rule >> { local_rules :: get_rules () } struct FromFileRunContextInstanceProviderFactoryLocal ; impl FromFileRunContextInstanceProviderFactory for FromFileRunContextInstanceProviderFactoryLocal { fn create < 'a > (& self) -> Box < dyn FromFileRunContextInstanceProvider < 'a > + 'a > { Box :: new (FromFileRunContextInstanceProviderLocal { tree_sitter_lint_plugin_replace_foo_with_provided_instances : tree_sitter_lint_plugin_replace_foo_with :: ProvidedTypes :: < 'a > :: once_lock_storage () }) } } struct FromFileRunContextInstanceProviderLocal < 'a > { tree_sitter_lint_plugin_replace_foo_with_provided_instances : < tree_sitter_lint_plugin_replace_foo_with :: ProvidedTypes :: < 'a > as FromFileRunContextProvidedTypes :: < 'a >> :: OnceLockStorage } impl < 'a > FromFileRunContextInstanceProvider < 'a > for FromFileRunContextInstanceProviderLocal < 'a > { fn get (& self , type_id : TypeId , file_run_context : FileRunContext < 'a , '_ > ,) -> Option < & dyn Tid < 'a >> { self . tree_sitter_lint_plugin_replace_foo_with_provided_instances . get (type_id , file_run_context) } }