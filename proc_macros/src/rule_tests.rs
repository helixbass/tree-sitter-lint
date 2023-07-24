use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, token, Expr, ExprArray, Ident, Token,
};

struct RuleOptions {
    options: Expr,
}

impl Parse for RuleOptions {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let options: Expr = input.parse()?;
        Ok(Self { options })
    }
}

impl ToTokens for RuleOptions {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let options = &self.options;
        quote! {
            #options.into()
        }
        .to_tokens(tokens)
    }
}

struct ValidRuleTestSpec {
    code: Expr,
    options: Option<RuleOptions>,
}

impl Parse for ValidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut options: Option<RuleOptions> = Default::default();
        if input.peek(token::Brace) {
            let content;
            braced!(content in input);
            while !content.is_empty() {
                let key: Ident = content.parse()?;
                content.parse::<Token![=>]>()?;
                match &*key.to_string() {
                    "code" => {
                        code = Some(content.parse()?);
                    }
                    "options" => {
                        options = Some(content.parse()?);
                    }
                    _ => panic!("didn't expect key '{}'", key),
                }
                if !content.is_empty() {
                    content.parse::<Token![,]>()?;
                }
            }
        } else {
            code = Some(input.parse()?);
        }
        Ok(Self {
            code: code.expect("Expected 'code'"),
            options,
        })
    }
}

impl ToTokens for ValidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let code = &self.code;
        let options = match self.options.as_ref() {
            Some(options) => quote! {
                Some(#options)
            },
            None => quote!(None),
        };
        quote! {
            tree_sitter_lint::RuleTestValid::new(
                #code,
                #options
            )
        }
        .to_tokens(tokens)
    }
}

struct InvalidRuleTestSpec {
    code: Expr,
    errors: ExprArray,
    output: Option<Expr>,
    options: Option<RuleOptions>,
}

impl Parse for InvalidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut errors: Option<ExprArray> = Default::default();
        let mut output: Option<Expr> = Default::default();
        let mut options: Option<RuleOptions> = Default::default();
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
                "options" => {
                    options = Some(content.parse()?);
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
            options,
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
        let options = match self.options.as_ref() {
            Some(options) => quote! {
                Some(#options)
            },
            None => quote!(None),
        };
        quote! {
            tree_sitter_lint::RuleTestInvalid::new(
                #code,
                #errors,
                #output,
                #options
            )
        }
        .to_tokens(tokens)
    }
}

struct RuleTests {
    valid: Vec<ValidRuleTestSpec>,
    invalid: Vec<InvalidRuleTestSpec>,
}

impl Parse for RuleTests {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut valid: Option<Vec<ValidRuleTestSpec>> = Default::default();
        let mut invalid: Option<Vec<InvalidRuleTestSpec>> = Default::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            match &*key.to_string() {
                "valid" => {
                    assert!(valid.is_none(), "Already saw 'valid' key");
                    let valid_content;
                    bracketed!(valid_content in input);
                    let valid = valid.get_or_insert_with(|| Default::default());
                    while !valid_content.is_empty() {
                        let valid_rule_test_spec: ValidRuleTestSpec = valid_content.parse()?;
                        valid.push(valid_rule_test_spec);
                        if !valid_content.is_empty() {
                            valid_content.parse::<Token![,]>()?;
                        }
                    }
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

pub fn rule_tests(input: TokenStream, crate_name: &str) -> TokenStream {
    let crate_name = format_ident!("{}", crate_name);
    let RuleTests { valid, invalid } = parse_macro_input!(input);

    quote! {
        {
            use #crate_name as tree_sitter_lint;

            tree_sitter_lint::RuleTests::new(
                vec![#(#valid),*],
                vec![#(#invalid),*],
            )
        }
    }
    .into()
}
