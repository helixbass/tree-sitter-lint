use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, token, Expr, Ident, Token,
};

use crate::shared::{parse_data, ExprOrIdent};

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

enum InvalidRuleTestErrorSpec {
    Fields {
        message: Option<Expr>,
        line: Option<Expr>,
        column: Option<Expr>,
        end_line: Option<Expr>,
        end_column: Option<Expr>,
        type_: Option<Expr>,
        message_id: Option<Expr>,
        data: Option<HashMap<ExprOrIdent, Expr>>,
    },
    Expr(Expr),
}

impl Parse for InvalidRuleTestErrorSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(token::Brace) {
            let error_content;
            braced!(error_content in input);
            let mut message: Option<Expr> = Default::default();
            let mut line: Option<Expr> = Default::default();
            let mut column: Option<Expr> = Default::default();
            let mut end_line: Option<Expr> = Default::default();
            let mut end_column: Option<Expr> = Default::default();
            let mut type_: Option<Expr> = Default::default();
            let mut message_id: Option<Expr> = Default::default();
            let mut data: Option<HashMap<ExprOrIdent, Expr>> = Default::default();
            while !error_content.is_empty() {
                let key: Result<Ident, _> = error_content.parse();
                let key = match key {
                    Ok(key) => key.to_string(),
                    Err(err) => {
                        if error_content.parse::<Token![type]>().is_ok() {
                            "type_".to_string()
                        } else {
                            return Err(err);
                        }
                    }
                };
                error_content.parse::<Token![=>]>()?;
                match &*key {
                    "message" => {
                        message = Some(error_content.parse()?);
                    }
                    "line" => {
                        line = Some(error_content.parse()?);
                    }
                    "column" => {
                        column = Some(error_content.parse()?);
                    }
                    "end_line" => {
                        end_line = Some(error_content.parse()?);
                    }
                    "end_column" => {
                        end_column = Some(error_content.parse()?);
                    }
                    "type_" => {
                        type_ = Some(error_content.parse()?);
                    }
                    "type" => {
                        type_ = Some(error_content.parse()?);
                    }
                    "message_id" => {
                        message_id = Some(error_content.parse()?);
                    }
                    "data" => {
                        assert!(data.is_none(), "already saw 'data' key");
                        let data = data.get_or_insert_with(Default::default);
                        parse_data(data, &error_content)?;
                    }
                    _ => panic!("didn't expect key '{}'", key),
                }
                if !error_content.is_empty() {
                    error_content.parse::<Token![,]>()?;
                }
            }
            Self::Fields {
                message,
                line,
                column,
                end_line,
                end_column,
                type_,
                message_id,
                data,
            }
        } else {
            Self::Expr(input.parse()?)
        })
    }
}

impl ToTokens for InvalidRuleTestErrorSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Fields {
                message,
                line,
                column,
                end_line,
                end_column,
                type_,
                message_id,
                data,
            } => {
                let message = match message.as_ref() {
                    Some(message) => quote!(Some(#message.into())),
                    None => quote!(None),
                };
                let line = match line.as_ref() {
                    Some(line) => quote!(Some(#line)),
                    None => quote!(None),
                };
                let column = match column.as_ref() {
                    Some(column) => quote!(Some(#column)),
                    None => quote!(None),
                };
                let end_line = match end_line.as_ref() {
                    Some(end_line) => quote!(Some(#end_line)),
                    None => quote!(None),
                };
                let end_column = match end_column.as_ref() {
                    Some(end_column) => quote!(Some(#end_column)),
                    None => quote!(None),
                };
                let type_ = match type_.as_ref() {
                    Some(type_) => quote!(Some(#type_.into())),
                    None => quote!(None),
                };
                let message_id = match message_id.as_ref() {
                    Some(message_id) => quote!(Some(#message_id.into())),
                    None => quote!(None),
                };
                let data = match data.as_ref() {
                    Some(data) => {
                        let data_keys = data.keys().map(|key| match key {
                            ExprOrIdent::Ident(key) => quote!(stringify!(#key)),
                            ExprOrIdent::Expr(Expr::Path(key))
                                if key.path.get_ident().is_some() =>
                            {
                                quote!(stringify!(#key))
                            }
                            _ => quote!(#key),
                        });
                        let data_values = data.values();
                        quote! {
                            Some([#((#data_keys.to_string(), #data_values.to_string())),*].into())
                        }
                    }
                    None => quote!(None),
                };
                quote! {
                    tree_sitter_lint::RuleTestExpectedError {
                        message: #message,
                        line: #line,
                        column: #column,
                        end_line: #end_line,
                        end_column: #end_column,
                        type_: #type_,
                        message_id: #message_id,
                        data: #data,
                    }
                }
            }
            Self::Expr(expr) => {
                quote!(tree_sitter_lint::RuleTestExpectedError::from(#expr))
            }
        }
        .to_tokens(tokens)
    }
}

struct InvalidRuleTestSpec {
    code: Expr,
    errors: Vec<InvalidRuleTestErrorSpec>,
    output: Option<Expr>,
    options: Option<RuleOptions>,
}

impl Parse for InvalidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut errors: Option<Vec<InvalidRuleTestErrorSpec>> = Default::default();
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
                    let errors_content;
                    bracketed!(errors_content in content);
                    let errors = errors.get_or_insert_with(|| Default::default());
                    while !errors_content.is_empty() {
                        errors.push(errors_content.parse()?);
                        if !errors_content.is_empty() {
                            errors_content.parse::<Token![,]>()?;
                        }
                    }
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
        let errors = self.errors.iter().map(|error| quote!(#error));
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
                vec![#(#errors),*],
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
                        invalid.push(invalid_content.parse()?);
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
