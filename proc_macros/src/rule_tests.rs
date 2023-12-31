use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    token, Expr, Ident, Token,
};

use crate::{
    helpers::ExprOrArrowSeparatedKeyValuePairs,
    shared::{parse_data, ExprOrIdent},
};

enum RuleOptions {
    Map(HashMap<ExprOrIdent, ExprOrArrowSeparatedKeyValuePairs>),
    List(Vec<ExprOrArrowSeparatedKeyValuePairs>),
    Expr(Expr),
}

impl Parse for RuleOptions {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(token::Bracket) {
            let list_input;
            bracketed!(list_input in input);
            let mut items: Vec<ExprOrArrowSeparatedKeyValuePairs> = Default::default();
            while !list_input.is_empty() {
                items.push(list_input.parse()?);
                if !list_input.is_empty() {
                    list_input.parse::<Token![,]>()?;
                }
            }
            Self::List(items)
        } else if input.peek(token::Brace) {
            let mut map: HashMap<ExprOrIdent, ExprOrArrowSeparatedKeyValuePairs> =
                Default::default();
            let data_content;
            braced!(data_content in input);
            while !data_content.is_empty() {
                let key: Result<Expr, _> = data_content.parse();
                let key: ExprOrIdent = match key {
                    Ok(key) => Ok(key.into()),
                    Err(err) => {
                        if let Ok(key) = data_content.parse::<Token![type]>() {
                            Ok(Ident::new("type_", key.span()).into())
                        } else {
                            Err(err)
                        }
                    }
                }?;
                data_content.parse::<Token![=>]>()?;
                let value: ExprOrArrowSeparatedKeyValuePairs = data_content.parse()?;
                map.insert(key, value);
                if !data_content.is_empty() {
                    data_content.parse::<Token![,]>()?;
                }
            }
            Self::Map(map)
        } else {
            let expr: Expr = input.parse()?;
            Self::Expr(expr)
        })
    }
}

impl ToTokens for RuleOptions {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let mut should_jsonify = true;
        let json = match self {
            RuleOptions::Map(map) => {
                let keys = map.keys().map(|key| {
                    match key {
                        ExprOrIdent::Expr(Expr::Path(path)) if path.path.get_ident().is_some() => {
                            let ident = path.path.get_ident().unwrap();
                            quote!(stringify!(#ident))
                        },
                        ExprOrIdent::Expr(key) => quote!(#key),
                        ExprOrIdent::Ident(key) => quote!(stringify!(#key)),
                    }
                });
                let values = map.values().map(|value| match value {
                    ExprOrArrowSeparatedKeyValuePairs::Expr(value) => quote!(#value),
                    ExprOrArrowSeparatedKeyValuePairs::ArrowSeparatedKeyValuePairs(value) => {
                        value.to_json()
                    }
                });
                quote! {
                    { #(#keys: #values),* }
                }
            }
            RuleOptions::List(list) => {
                let items = list.into_iter().map(|item| item.to_json());
                quote! {
                    [ #(#items),* ]
                }
            }
            RuleOptions::Expr(Expr::Path(path)) if path.path.get_ident().is_some() => {
                should_jsonify = false;
                let ident = path.path.get_ident().unwrap();
                quote!(#ident.clone())
            }
            RuleOptions::Expr(expr) => {
                quote!(#expr)
            }
        };
        if should_jsonify {
            quote! {
                tree_sitter_lint::serde_json::json!(#json)
            }
        } else {
            json
        }
        .to_tokens(tokens)
    }
}

enum ValidRuleTestSpec {
    Single(SingleValidRuleTestSpec),
    Spread(Expr),
}

impl Parse for ValidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            input.parse::<Token![.]>()?;
            input.parse::<Token![.]>()?;
            Self::Spread(input.parse()?)
        } else {
            Self::Single(input.parse()?)
        })
    }
}

impl ToTokens for ValidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Single(value) => value.to_tokens(tokens),
            Self::Spread(value) => value.to_tokens(tokens),
        }
    }
}

struct SingleValidRuleTestSpec {
    code: Expr,
    options: Option<RuleOptions>,
    only: Option<Expr>,
    environment: Option<ExprOrArrowSeparatedKeyValuePairs>,
    supported_language_languages: Option<Vec<Ident>>,
}

impl Parse for SingleValidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut options: Option<RuleOptions> = Default::default();
        let mut only: Option<Expr> = Default::default();
        let mut environment: Option<ExprOrArrowSeparatedKeyValuePairs> = Default::default();
        let mut supported_language_languages: Option<Vec<Ident>> = Default::default();
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
                    "only" => {
                        only = Some(content.parse()?);
                    }
                    "environment" => {
                        environment = Some(content.parse()?);
                    }
                    "supported_language_languages" => {
                        assert!(supported_language_languages.is_none(), "Already saw 'supported_language_languages' key");
                        let supported_language_languages_content;
                        bracketed!(supported_language_languages_content in content);
                        let supported_language_languages = supported_language_languages.get_or_insert_with(|| Default::default());
                        while !supported_language_languages_content.is_empty() {
                            supported_language_languages.push(supported_language_languages_content.parse()?);
                            if !supported_language_languages_content.is_empty() {
                                supported_language_languages_content.parse::<Token![,]>()?;
                            }
                        }
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
            only,
            environment,
            supported_language_languages,
        })
    }
}

impl ToTokens for SingleValidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let code = &self.code;
        let options = match self.options.as_ref() {
            Some(options) => quote! {
                Some(#options)
            },
            None => quote!(None),
        };
        let only = match self.only.as_ref() {
            Some(only) => quote! {
                Some(#only)
            },
            None => quote!(None),
        };
        let environment = match self.environment.as_ref() {
            Some(ExprOrArrowSeparatedKeyValuePairs::Expr(environment)) => quote!(Some(#environment)),
            Some(ExprOrArrowSeparatedKeyValuePairs::ArrowSeparatedKeyValuePairs(environment)) => {
                let environment = environment.to_json();
                quote! {
                    Some(match tree_sitter_lint::serde_json::json!(#environment) {
                        tree_sitter_lint::serde_json::Value::Object(environment) => environment,
                        _ => unreachable!(),
                    })
                }
            },
            None => quote!(None),
        };
        let supported_language_languages = match self.supported_language_languages.as_ref() {
            Some(supported_language_languages) => quote! {
                Some(vec![#(tree_sitter_lint::tree_sitter_grep::SupportedLanguageLanguage::#supported_language_languages),*])
            },
            None => quote!(None),
        };
        quote! {
            tree_sitter_lint::RuleTestValid::new(
                #code,
                #options,
                #only,
                #environment,
                #supported_language_languages
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
                quote!(tree_sitter_lint::RuleTestExpectedError::from(#expr.clone()))
            }
        }
        .to_tokens(tokens)
    }
}

fn expr_is_ident(expr: &Expr, ident_name: &str) -> bool {
    matches!(
        expr,
        Expr::Path(expr_path) if matches!(
            expr_path.path.get_ident(),
            Some(ident) if ident.to_string() == ident_name
        )
    )
}

enum InvalidRuleTestErrorsSpec {
    Expr(Expr),
    Vec(Vec<InvalidRuleTestErrorSpec>),
}

impl Parse for InvalidRuleTestErrorsSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(token::Bracket) {
            let mut errors: Vec<InvalidRuleTestErrorSpec> = Default::default();
            let errors_content;
            bracketed!(errors_content in input);
            while !errors_content.is_empty() {
                errors.push(errors_content.parse()?);
                if !errors_content.is_empty() {
                    errors_content.parse::<Token![,]>()?;
                }
            }
            Self::Vec(errors)
        } else {
            Self::Expr(input.parse()?)
        })
    }
}

impl ToTokens for InvalidRuleTestErrorsSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Expr(Expr::Lit(value)) => quote!(#value),
            Self::Expr(value) => quote!(#value.iter().cloned().collect::<Vec<_>>()),
            Self::Vec(value) => {
                let errors = value.iter().map(|error| quote!(#error));
                quote! {
                    vec![#(#errors),*]
                }
            }
        }
        .to_tokens(tokens)
    }
}

enum InvalidRuleTestSpec {
    Single(SingleInvalidRuleTestSpec),
    Spread(Expr),
}

impl Parse for InvalidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            input.parse::<Token![.]>()?;
            input.parse::<Token![.]>()?;
            Self::Spread(input.parse()?)
        } else {
            Self::Single(input.parse()?)
        })
    }
}

impl ToTokens for InvalidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Single(value) => value.to_tokens(tokens),
            Self::Spread(value) => value.to_tokens(tokens),
        }
    }
}

struct SingleInvalidRuleTestSpec {
    code: Expr,
    errors: InvalidRuleTestErrorsSpec,
    output: Option<Expr>,
    options: Option<RuleOptions>,
    only: Option<Expr>,
    environment: Option<ExprOrArrowSeparatedKeyValuePairs>,
    supported_language_languages: Option<Vec<Ident>>,
}

impl Parse for SingleInvalidRuleTestSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut code: Option<Expr> = Default::default();
        let mut errors: Option<InvalidRuleTestErrorsSpec> = Default::default();
        let mut output: Option<Expr> = Default::default();
        let mut options: Option<RuleOptions> = Default::default();
        let mut only: Option<Expr> = Default::default();
        let mut environment: Option<ExprOrArrowSeparatedKeyValuePairs> = Default::default();
        let mut supported_language_languages: Option<Vec<Ident>> = Default::default();
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
                "only" => {
                    only = Some(content.parse()?);
                }
                "environment" => {
                    environment = Some(content.parse()?);
                }
                "supported_language_languages" => {
                    assert!(supported_language_languages.is_none(), "Already saw 'supported_language_languages' key");
                    let supported_language_languages_content;
                    bracketed!(supported_language_languages_content in content);
                    let supported_language_languages = supported_language_languages.get_or_insert_with(|| Default::default());
                    while !supported_language_languages_content.is_empty() {
                        supported_language_languages.push(supported_language_languages_content.parse()?);
                        if !supported_language_languages_content.is_empty() {
                            supported_language_languages_content.parse::<Token![,]>()?;
                        }
                    }
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
            only,
            environment,
            supported_language_languages,
        })
    }
}

impl ToTokens for SingleInvalidRuleTestSpec {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let code = &self.code;
        let errors = &self.errors;
        let output = match self.output.as_ref() {
            Some(output) if expr_is_ident(output, "None") => quote! {
                Some(tree_sitter_lint::RuleTestExpectedOutput::NoOutput)
            },
            Some(output) => quote! {
                Some(#output)
            },
            None => quote!(Option::<tree_sitter_lint::RuleTestExpectedOutput>::None),
        };
        let options = match self.options.as_ref() {
            Some(options) => quote! {
                Some(#options)
            },
            None => quote!(None),
        };
        let only = match self.only.as_ref() {
            Some(only) => quote! {
                Some(#only)
            },
            None => quote!(None),
        };
        let environment = match self.environment.as_ref() {
            Some(ExprOrArrowSeparatedKeyValuePairs::Expr(environment)) => quote!(Some(#environment)),
            Some(ExprOrArrowSeparatedKeyValuePairs::ArrowSeparatedKeyValuePairs(environment)) => {
                let environment = environment.to_json();
                quote! {
                    Some(match tree_sitter_lint::serde_json::json!(#environment) {
                        tree_sitter_lint::serde_json::Value::Object(environment) => environment,
                        _ => unreachable!(),
                    })
                }
            },
            None => quote!(None),
        };
        let supported_language_languages = match self.supported_language_languages.as_ref() {
            Some(supported_language_languages) => quote! {
                Some(vec![#(tree_sitter_lint::tree_sitter_grep::SupportedLanguageLanguage::#supported_language_languages),*])
            },
            None => quote!(None),
        };
        quote! {
            tree_sitter_lint::RuleTestInvalid::new(
                #code,
                #errors,
                #output,
                #options,
                #only,
                #environment,
                #supported_language_languages,
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

    let valid = if valid
        .iter()
        .any(|valid_test| matches!(valid_test, ValidRuleTestSpec::Spread(_)))
    {
        let add_cases = valid
            .iter()
            .map(|valid_test| match valid_test {
                ValidRuleTestSpec::Single(value) => {
                    quote! {
                        cases.push(#value);
                    }
                }
                ValidRuleTestSpec::Spread(value) => {
                    quote! {
                        cases.extend(#value.into_iter().map(#crate_name::RuleTestValid::from));
                    }
                }
            })
            .collect::<Vec<_>>();
        quote! {{
            let mut cases: Vec<#crate_name::RuleTestValid> = vec![];
            #(#add_cases)*
            cases
        }}
    } else {
        quote!(vec![#(#valid),*])
    };

    let invalid = if invalid
        .iter()
        .any(|invalid_test| matches!(invalid_test, InvalidRuleTestSpec::Spread(_)))
    {
        let add_cases = invalid
            .iter()
            .map(|invalid_test| match invalid_test {
                InvalidRuleTestSpec::Single(value) => {
                    quote! {
                        cases.push(#value);
                    }
                }
                InvalidRuleTestSpec::Spread(value) => {
                    quote! {
                        cases.extend(#value);
                    }
                }
            })
            .collect::<Vec<_>>();
        quote! {{
            let mut cases: Vec<#crate_name::RuleTestInvalid> = vec![];
            #(#add_cases)*
            cases
        }}
    } else {
        quote!(vec![#(#invalid),*])
    };

    quote! {
        {
            use #crate_name as tree_sitter_lint;

            tree_sitter_lint::RuleTests::new(
                #valid,
                #invalid,
            )
        }
    }
    .into()
}
