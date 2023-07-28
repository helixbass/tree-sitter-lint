use proc_macro::TokenStream;

mod builder_args;
mod helpers;
mod rule;
mod rule_tests;
mod violation;

use helpers::ArrowSeparatedKeyValuePairs;
use rule::rule_with_crate_name;
use violation::violation_with_crate_name;

#[proc_macro]
pub fn builder_args(input: TokenStream) -> TokenStream {
    builder_args::builder_args(input)
}

#[proc_macro]
pub fn rule_tests(input: TokenStream) -> TokenStream {
    rule_tests::rule_tests(input, "tree_sitter_lint")
}

#[proc_macro]
pub fn rule_tests_crate_internal(input: TokenStream) -> TokenStream {
    rule_tests::rule_tests(input, "crate")
}

#[proc_macro]
pub fn rule(input: TokenStream) -> TokenStream {
    rule_with_crate_name(input, "tree_sitter_lint")
}

#[proc_macro]
pub fn rule_crate_internal(input: TokenStream) -> TokenStream {
    rule_with_crate_name(input, "crate")
}

#[proc_macro]
pub fn violation(input: TokenStream) -> TokenStream {
    violation_with_crate_name(input, "tree_sitter_lint")
}

#[proc_macro]
pub fn violation_crate_internal(input: TokenStream) -> TokenStream {
    violation_with_crate_name(input, "crate")
}
