use std::collections::HashMap;

use inflector::Inflector;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream, Parser},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token,
    visit_mut::{self, VisitMut},
    Expr, ExprArray, ExprClosure, ExprField, ExprMacro, ExprPath, Ident, Member, Token, Type,
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
    capture_name: Option<Expr>,
    callback: ExprClosure,
}

impl Parse for RuleListenerSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let query: Expr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let callback: ExprClosure = input.parse()?;
        Ok(Self {
            query,
            callback,
            // TODO: figure out a syntax for this
            capture_name: None,
        })
    }
}

struct Rule {
    name: Expr,
    fixable: Option<Expr>,
    state: Option<RuleStateSpec>,
    listeners: Vec<RuleListenerSpec>,
}

impl Rule {
    pub fn name_string(&self) -> String {
        match &self.name {
            Expr::Path(value) => value.path.get_ident(),
            _ => None,
        }
        .map_or_else(|| "GeneratedRule".to_owned(), |ident| ident.to_string())
    }

    pub fn get_rule_state_scope_for_field(&self, field_name: &str) -> Option<RuleStateScope> {
        self.state.as_ref().and_then(|state| {
            state
                .scope_sections
                .iter()
                .find(|scope_section| {
                    scope_section
                        .fields
                        .iter()
                        .any(|field| field.name.to_string() == field_name)
                })
                .map(|scope_section| scope_section.scope)
        })
    }
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

    let rule_struct_name = format_ident!("{}", rule.name_string().to_class_case());

    let rule_state_fields = rule.state.as_ref().map_or_else(
        || Default::default(),
        |state| {
            state
                .scope_sections
                .iter()
                .filter(|scope_section| scope_section.scope == RuleStateScope::RuleStatic)
                .flat_map(|scope_section| scope_section.fields.iter())
                .collect::<Vec<_>>()
        },
    );

    let rule_struct_definition = get_rule_struct_definition(&rule_struct_name, &rule_state_fields);

    let rule_instance_struct_name = format_ident!("{}Instance", rule_struct_name);

    let rule_instance_state_fields = rule.state.as_ref().map_or_else(
        || Default::default(),
        |state| {
            state
                .scope_sections
                .iter()
                .filter(|scope_section| scope_section.scope == RuleStateScope::PerRun)
                .flat_map(|scope_section| scope_section.fields.iter())
                .collect::<Vec<_>>()
        },
    );

    let rule_rule_impl = get_rule_rule_impl(
        &rule,
        &rule_struct_name,
        &rule_instance_struct_name,
        &rule_instance_state_fields,
    );

    let rule_instance_struct_definition = get_rule_instance_struct_definition(
        &rule_struct_name,
        &rule_instance_struct_name,
        &rule_instance_state_fields,
    );

    let rule_instance_per_file_struct_name = format_ident!("{}PerFile", rule_instance_struct_name);

    let rule_instance_per_file_state_fields = rule.state.as_ref().map_or_else(
        || Default::default(),
        |state| {
            state
                .scope_sections
                .iter()
                .filter(|scope_section| scope_section.scope == RuleStateScope::PerFileRun)
                .flat_map(|scope_section| scope_section.fields.iter())
                .collect::<Vec<_>>()
        },
    );

    let rule_instance_rule_instance_impl = get_rule_instance_rule_instance_impl(
        &rule_instance_struct_name,
        &rule_instance_per_file_struct_name,
        &rule_instance_per_file_state_fields,
    );

    let rule_instance_per_file_struct_definition = get_rule_instance_per_file_struct_definition(
        &rule_instance_struct_name,
        &rule_instance_per_file_struct_name,
        &rule_instance_per_file_state_fields,
    );

    let rule_instance_per_file_rule_instance_per_file_impl =
        get_rule_instance_per_file_rule_instance_per_file_impl(
            &rule,
            &rule_instance_per_file_struct_name,
        );

    let instantiate_rule = get_rule_struct_creation(&rule_struct_name, &rule_state_fields);

    quote! {
        {
            #rule_struct_definition

            #rule_rule_impl

            #rule_instance_struct_definition

            #rule_instance_rule_instance_impl

            #rule_instance_per_file_struct_definition

            #rule_instance_per_file_rule_instance_per_file_impl

            #instantiate_rule
        }
    }
    .into()
}

fn get_rule_struct_definition(
    rule_struct_name: &Ident,
    rule_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let field_names = rule_state_fields.iter().map(|field| &field.name);
    let field_types = rule_state_fields.iter().map(|field| &field.type_);
    quote! {
        struct #rule_struct_name {
            #(#field_names: #field_types),*
        }
    }
}

fn get_rule_rule_impl(
    rule: &Rule,
    rule_struct_name: &Ident,
    rule_instance_struct_name: &Ident,
    rule_instance_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let name = &rule.name;
    let fixable = match rule.fixable.as_ref() {
        Some(fixable) => quote!(#fixable),
        None => quote!(false),
    };
    let rule_instance_state_field_names =
        rule_instance_state_fields.iter().map(|field| &field.name);
    let rule_instance_state_field_initializers =
        rule_instance_state_fields
            .iter()
            .map(|field| match field.initializer.as_ref() {
                Some(initializer) => quote!(#initializer),
                None => quote!(Default::default()),
            });
    let rule_listener_queries = rule.listeners.iter().map(|listener| &listener.query);
    let rule_listener_capture_names =
        rule.listeners
            .iter()
            .map(|listener| match listener.capture_name.as_ref() {
                Some(capture_name) => quote!(Some(#capture_name)),
                None => quote!(None),
            });
    quote! {
        impl crate::rule::Rule for #rule_struct_name {
            fn meta(&self) -> crate::rule::RuleMeta {
                crate::rule::RuleMeta {
                    name: #name,
                    fixable: #fixable,
                    languages: vec![tree_sitter_grep::SupportedLanguage::Rust],
                }
            }

            fn instantiate(self: std::sync::Arc<Self>, _config: &crate::config::Config) -> std::sync::Arc<dyn crate::rule::RuleInstance> {
                std::sync::Arc::new(#rule_instance_struct_name {
                    rule: self.clone(),
                    listener_queries: vec![
                        #(crate::rule::RuleListenerQuery {
                            query: #rule_listener_queries,
                            capture_name: #rule_listener_capture_names,
                        }),*
                    ],
                    #(#rule_instance_state_field_names: #rule_instance_state_field_initializers),*
                })
            }
        }
    }
}

fn get_rule_instance_struct_definition(
    rule_struct_name: &Ident,
    rule_instance_struct_name: &Ident,
    rule_instance_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let state_field_names = rule_instance_state_fields.iter().map(|field| &field.name);
    let state_field_types = rule_instance_state_fields.iter().map(|field| &field.type_);
    quote! {
        struct #rule_instance_struct_name {
            rule: std::sync::Arc<#rule_struct_name>,
            listener_queries: Vec<crate::rule::RuleListenerQuery>,
            #(#state_field_names: #state_field_types),*
        }
    }
}

fn get_rule_instance_rule_instance_impl(
    rule_instance_struct_name: &Ident,
    rule_instance_per_file_struct_name: &Ident,
    rule_instance_per_file_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let rule_instance_per_file_state_field_names = rule_instance_per_file_state_fields
        .iter()
        .map(|field| &field.name);
    let rule_instance_per_file_state_field_initializers = rule_instance_per_file_state_fields
        .iter()
        .map(|field| match field.initializer.as_ref() {
            Some(initializer) => quote!(#initializer),
            None => quote!(Default::default()),
        });
    quote! {
        impl crate::rule::RuleInstance for #rule_instance_struct_name {
            fn instantiate_per_file(
                self: std::sync::Arc<Self>,
                _file_run_info: &crate::rule::FileRunInfo,
            ) -> Arc<dyn crate::rule::RuleInstancePerFile> {
                std::sync::Arc::new(#rule_instance_per_file_struct_name {
                    rule_instance: self,
                    #(#rule_instance_per_file_state_field_names: #rule_instance_per_file_state_field_initializers),*
                })
            }

            fn rule(&self) -> Arc<dyn crate::rule::Rule> {
                self.rule.clone()
            }

            fn listener_queries(&self) -> &[crate::rule::RuleListenerQuery] {
                &self.listener_queries
            }
        }
    }
}

fn get_rule_instance_per_file_struct_definition(
    rule_instance_struct_name: &Ident,
    rule_instance_per_file_struct_name: &Ident,
    rule_instance_per_file_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let state_field_names = rule_instance_per_file_state_fields
        .iter()
        .map(|field| &field.name);
    let state_field_types = rule_instance_per_file_state_fields
        .iter()
        .map(|field| &field.type_);
    quote! {
        struct #rule_instance_per_file_struct_name {
            rule_instance: std::sync::Arc<#rule_instance_struct_name>,
            #(#state_field_names: #state_field_types),*
        }
    }
}

struct SelfAccessRewriter<'a> {
    rule: &'a Rule,
}

impl<'a> visit_mut::VisitMut for SelfAccessRewriter<'a> {
    fn visit_expr_field_mut(&mut self, node: &mut ExprField) {
        if let Some(self_field_name) = get_self_field_access_name(node) {
            match self.rule.get_rule_state_scope_for_field(&self_field_name) {
                Some(RuleStateScope::RuleStatic) => {
                    let self_field_name = format_ident!("{}", self_field_name);
                    *node = parse_quote!(self.rule_instance.rule.#self_field_name);
                    return;
                }
                Some(RuleStateScope::PerRun) => {
                    let self_field_name = format_ident!("{}", self_field_name);
                    *node = parse_quote!(self.rule.#self_field_name);
                    return;
                }
                _ => (),
            }
        }
        visit_mut::visit_expr_field_mut(self, node);
    }

    fn visit_expr_macro_mut(&mut self, node: &mut ExprMacro) {
        let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
        let rewritten_macro_args = match parser.parse2(node.mac.tokens.clone()) {
            Ok(macro_args) => macro_args,
            _ => return,
        }
        .into_iter()
        .map(|mut macro_arg| {
            SelfAccessRewriter { rule: self.rule }.visit_expr_mut(&mut macro_arg);
            macro_arg
        })
        .collect::<Vec<_>>();
        node.mac.tokens = quote! {
            #(#rewritten_macro_args),*
        }
        .into();
        visit_mut::visit_expr_macro_mut(self, node);
    }
}

fn get_self_field_access_name(expr_field: &ExprField) -> Option<String> {
    if !matches!(
        &*expr_field.base,
        Expr::Path(base) if matches!(
            base.path.get_ident(),
            Some(base) if base.to_string() == "self"
        )
    ) {
        return None;
    }
    match &expr_field.member {
        Member::Named(member) => Some(member.to_string()),
        _ => None,
    }
}

fn get_rule_instance_per_file_rule_instance_per_file_impl(
    rule: &Rule,
    rule_instance_per_file_struct_name: &Ident,
) -> proc_macro2::TokenStream {
    let listener_indices = 0..rule.listeners.len();
    let listener_callbacks = rule.listeners.iter().map(|listener| {
        let mut callback_body = listener.callback.body.clone();
        SelfAccessRewriter { rule }.visit_expr_mut(&mut callback_body);
        callback_body
    });
    quote! {
        impl crate::rule::RuleInstancePerFile for #rule_instance_per_file_struct_name {
            fn on_query_match(&self, listener_index: usize, node: Node, context: &mut QueryMatchContext) {
                match listener_index {
                    #(#listener_indices => {
                        #listener_callbacks
                    })*
                    _ => unreachable!(),
                }
            }

            fn rule_instance(&self) -> std::sync::Arc<dyn crate::rule::RuleInstance> {
                self.rule_instance.clone()
            }
        }
    }
}

fn get_rule_struct_creation(
    rule_struct_name: &Ident,
    rule_state_fields: &[&RuleStateFieldSpec],
) -> proc_macro2::TokenStream {
    let rule_state_field_names = rule_state_fields.iter().map(|field| &field.name);
    let rule_state_field_initializers =
        rule_state_fields
            .iter()
            .map(|field| match field.initializer.as_ref() {
                Some(initializer) => quote!(#initializer),
                None => quote!(Default::default()),
            });
    quote! {
        #rule_struct_name {
            #(#rule_state_field_names: #rule_state_field_initializers),*
        }
    }
}
