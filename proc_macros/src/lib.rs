use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, token, Expr, ExprArray, ExprClosure, ExprPath, Ident, Token, Type,
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
            crate::RuleTestInvalid::new(
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
        crate::RuleTests::new(
            #valid,
            vec![#(#invalid),*],
        )
    }
    .into()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RuleStateScope {
    RuleStatic,
    PerRun,
    PerFileRun,
}

impl Parse for RuleStateScope {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        bracketed!(content in input);
        let found = match &*content.parse::<Ident>()?.to_string() {
            "rule" => {
                content.parse::<Token![-]>()?;
                content.parse::<Token![static]>()?;
                Self::RuleStatic
            }
            "per" => {
                content.parse::<Token![-]>()?;
                match &*content.parse::<Ident>()?.to_string() {
                    "run" => Self::PerRun,
                    "file" => {
                        content.parse::<Token![-]>()?;
                        match &*content.parse::<Ident>()?.to_string() {
                            "run" => Self::PerFileRun,
                            _ => {
                                return Err(
                                    content.error("Expected rule-static, per-run or per-file-run")
                                )
                            }
                        }
                    }
                    _ => return Err(content.error("Expected rule-static, per-run or per-file-run")),
                }
            }
            _ => return Err(content.error("Expected rule-static, per-run or per-file-run")),
        };
        if !content.is_empty() {
            return Err(content.error("Expected rule-static, per-run or per-file-run"));
        }
        Ok(found)
    }
}

struct RuleStateFieldSpec {
    name: Ident,
    type_: Type,
    initializer: Option<Expr>,
}

impl Parse for RuleStateFieldSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let type_: Type = input.parse()?;
        let mut initializer: Option<Expr> = Default::default();
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            initializer = Some(input.parse()?);
        }
        Ok(Self {
            name,
            type_,
            initializer,
        })
    }
}

struct RuleStateScopeSection {
    scope: RuleStateScope,
    fields: Vec<RuleStateFieldSpec>,
}

impl Parse for RuleStateScopeSection {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let scope: RuleStateScope = input.parse()?;
        let mut fields: Vec<RuleStateFieldSpec> = Default::default();
        while !input.is_empty() && !input.peek(token::Bracket) {
            fields.push(input.parse()?);
            if !input.is_empty() {
                let _ = input.parse::<Token![,]>();
            }
        }
        Ok(Self { scope, fields })
    }
}

struct RuleStateSpec {
    scope_sections: Vec<RuleStateScopeSection>,
}

impl Parse for RuleStateSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut scope_sections: Vec<RuleStateScopeSection> = Default::default();
        let rule_state_spec_content;
        braced!(rule_state_spec_content in input);
        while !rule_state_spec_content.is_empty() {
            scope_sections.push(rule_state_spec_content.parse()?);
            if !rule_state_spec_content.is_empty() {
                let _ = rule_state_spec_content.parse::<Token![,]>();
            }
        }
        Ok(Self { scope_sections })
    }
}

struct RuleListenerSpec {
    query: Expr,
    callback: ExprClosure,
}

impl Parse for RuleListenerSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let query: Expr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let callback: ExprClosure = input.parse()?;
        Ok(Self { query, callback })
    }
}

struct Rule {
    name: Expr,
    fixable: Option<Expr>,
    state: Option<RuleStateSpec>,
    listeners: Vec<RuleListenerSpec>,
}

impl Parse for Rule {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<Expr> = Default::default();
        let mut fixable: Option<Expr> = Default::default();
        let mut state: Option<RuleStateSpec> = Default::default();
        let mut listeners: Option<Vec<RuleListenerSpec>> = Default::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            match &*key.to_string() {
                "name" => {
                    assert!(name.is_none(), "Already saw 'name' key");
                    name = Some(input.parse()?);
                }
                "fixable" => {
                    assert!(fixable.is_none(), "Already saw 'fixable' key");
                    fixable = Some(input.parse()?);
                }
                "state" => {
                    assert!(state.is_none(), "Already saw 'state' key");
                    state = Some(input.parse()?);
                }
                "listeners" => {
                    assert!(listeners.is_none(), "Already saw 'listeners' key");
                    let listeners_content;
                    bracketed!(listeners_content in input);
                    let listeners = listeners.get_or_insert_with(|| Default::default());
                    while !listeners_content.is_empty() {
                        let rule_listener_spec: RuleListenerSpec = listeners_content.parse()?;
                        listeners.push(rule_listener_spec);
                        if !listeners_content.is_empty() {
                            listeners_content.parse::<Token![,]>()?;
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
            name: name.expect("Expected 'name'"),
            fixable,
            state,
            listeners: listeners.expect("Expected 'listeners'"),
        })
    }
}

#[proc_macro]
pub fn rule(input: TokenStream) -> TokenStream {
    let rule: Rule = parse_macro_input!(input);
    unimplemented!()
}
