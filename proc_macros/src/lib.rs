use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, ExprArray, ExprPath, Ident, Token,
};

struct BuilderArgs {
    builder_name: ExprPath,
    args: HashMap<Ident, Expr>,
}

impl Parse for BuilderArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let builder_name: ExprPath = input.parse()?;
        input.parse::<Token![,]>()?;
        let mut args: HashMap<Ident, Expr> = Default::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            let value: Expr = input.parse()?;
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
            args.insert(key, value);
        }
        Ok(BuilderArgs { builder_name, args })
    }
}

#[proc_macro]
pub fn builder_args(input: TokenStream) -> TokenStream {
    let BuilderArgs { builder_name, args } = parse_macro_input!(input as BuilderArgs);

    let keys = args.keys();
    let values = args.values();
    quote! {
        #builder_name::default()
            #(.#keys(#values))*
            .build()
            .unwrap()
    }
    .into()
}

struct InvalidRuleTestSpec {
    code: Expr,
    errors: ExprArray,
    output: Option<Expr>,
}

impl Parse for InvalidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut errors: Option<ExprArray> = Default::default();
        let mut output: Option<Expr> = Default::default();
        let content;
        braced!(content in input);
        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![=>]>()?;
            match &*key.to_string() {
                "code" => {
                    code = Some(content.parse()?);
                }
                "errors" => {
                    errors = Some(content.parse()?);
                }
                "output" => {
                    output = Some(content.parse()?);
                }
                _ => panic!("didn't expect key '{}'", key),
            }
            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            code: code.expect("Expected 'code'"),
            errors: errors.expect("Expected 'errors'"),
            output,
        })
    }
}

impl ToTokens for InvalidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let code = &self.code;
        let errors = &self.errors;
        let output = match self.output.as_ref() {
            Some(output) => quote! {
                Some(#output)
            },
            None => quote! {
                Option::<String>::None
            },
        };
        quote! {
            tree_sitter_lint::RuleTestInvalid::new(
                #code,
                #errors,
                #output
            )
        }
        .to_tokens(tokens)
    }
}

struct RuleTests {
    valid: ExprArray,
    invalid: Vec<InvalidRuleTestSpec>,
}

impl Parse for RuleTests {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut valid: Option<ExprArray> = Default::default();
        let mut invalid: Option<Vec<InvalidRuleTestSpec>> = Default::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            match &*key.to_string() {
                "valid" => {
                    assert!(valid.is_none(), "Already saw 'valid' key");
                    valid = Some(input.parse()?);
                }
                "invalid" => {
                    assert!(invalid.is_none(), "Already saw 'invalid' key");
                    let invalid_content;
                    bracketed!(invalid_content in input);
                    let invalid = invalid.get_or_insert_with(|| Default::default());
                    while !invalid_content.is_empty() {
                        let invalid_rule_test_spec: InvalidRuleTestSpec =
                            invalid_content.parse()?;
                        invalid.push(invalid_rule_test_spec);
                        if !invalid_content.is_empty() {
                            invalid_content.parse::<Token![,]>()?;
                        }
                    }
                }
                _ => panic!("didn't expect key '{}'", key),
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            valid: valid.expect("Expected 'valid'"),
            invalid: invalid.expect("Expected 'invalid'"),
        })
    }
}

#[proc_macro]
pub fn rule_tests(input: TokenStream) -> TokenStream {
    let RuleTests { valid, invalid } = parse_macro_input!(input);

    quote! {
        tree_sitter_lint::RuleTests::new(
            #valid,
            vec![#(#invalid),*],
        )
    }
    .into()
}
