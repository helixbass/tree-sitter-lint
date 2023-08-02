use std::collections::HashMap;

use inflector::Inflector;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream, Parser},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token,
    visit_mut::{self, VisitMut},
    Expr, ExprClosure, ExprField, ExprMacro, Ident, Member, Pat, PathArguments, Token, Type,
};

use crate::{helpers::ExprOrArrowSeparatedKeyValuePairs, ArrowSeparatedKeyValuePairs};

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

impl RuleListenerSpec {
    pub fn is_per_match(&self) -> bool {
        matches!(
            self.callback.inputs.iter().next(),
            Some(Pat::Ident(first_param)) if first_param.ident.to_string() == "captures"
        )
    }
}

impl Parse for RuleListenerSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let query: Expr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let mut callback: Option<ExprClosure> = Default::default();
        let mut capture_name: Option<Expr> = Default::default();
        match input.parse::<ExprClosure>() {
            Ok(parsed) => callback = Some(parsed),
            _ => {
                let content;
                braced!(content in input);
                while !content.is_empty() {
                    let key: Ident = content.parse()?;
                    content.parse::<Token![=>]>()?;
                    match &*key.to_string() {
                        "callback" => {
                            callback = Some(content.parse()?);
                        }
                        "capture_name" => {
                            capture_name = Some(content.parse()?);
                        }
                        key => panic!("Unexpected key: '{}'", key),
                    }
                    if !content.is_empty() {
                        content.parse::<Token![,]>()?;
                    }
                }
            }
        }
        Ok(Self {
            query,
            callback: callback.expect("Expected 'callback'"),
            capture_name,
        })
    }
}

struct Rule {
    name: Expr,
    fixable: Option<Expr>,
    state: Option<RuleStateSpec>,
    listeners: Vec<RuleListenerSpec>,
    options_type: Option<Type>,
    languages: Vec<Ident>,
    messages: Option<HashMap<Expr, Expr>>,
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
        let mut options_type: Option<Type> = Default::default();
        let mut languages: Option<Vec<Ident>> = Default::default();
        let mut messages: Option<HashMap<Expr, Expr>> = Default::default();
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
                "options_type" => {
                    assert!(options_type.is_none(), "Already saw 'options_type' key");
                    options_type = Some(input.parse()?);
                }
                "languages" => {
                    assert!(languages.is_none(), "Already saw 'languages' key");
                    let languages_content;
                    bracketed!(languages_content in input);
                    let languages = languages.get_or_insert_with(|| Default::default());
                    while !languages_content.is_empty() {
                        languages.push(languages_content.parse()?);
                        if !languages_content.is_empty() {
                            languages_content.parse::<Token![,]>()?;
                        }
                    }
                }
                "messages" => {
                    assert!(messages.is_none(), "Already saw 'messages' key");
                    let messages_content;
                    bracketed!(messages_content in input);
                    let messages = messages.get_or_insert_with(|| Default::default());
                    while !messages_content.is_empty() {
                        let key: Expr = messages_content.parse()?;
                        messages_content.parse::<Token![=>]>()?;
                        let value: Expr = messages_content.parse()?;
                        messages.insert(key, value);
                        if !messages_content.is_empty() {
                            messages_content.parse::<Token![,]>()?;
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
            options_type,
            languages: languages.expect("Expected 'languages'"),
            messages,
        })
    }
}

pub fn rule_with_crate_name(input: TokenStream, crate_name: &str) -> TokenStream {
    let crate_name = format_ident!("{}", crate_name);
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
        &crate_name,
    );

    let rule_instance_struct_definition = get_rule_instance_struct_definition(
        &rule_struct_name,
        &rule_instance_struct_name,
        &rule_instance_state_fields,
        &crate_name,
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
        &crate_name,
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
            &crate_name,
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

fn is_option_type(type_: &Type) -> bool {
    match type_ {
        Type::Path(type_path) => matches!(
            type_path.path.segments.first(),
            Some(first_segment) if matches!(
                &first_segment.arguments,
                PathArguments::AngleBracketed(_)
            ) && first_segment.ident.to_string() == "Option"
        ),
        _ => false,
    }
}

fn get_rule_rule_impl(
    rule: &Rule,
    rule_struct_name: &Ident,
    rule_instance_struct_name: &Ident,
    rule_instance_state_fields: &[&RuleStateFieldSpec],
    crate_name: &Ident,
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
    let rule_listener_match_bys = rule.listeners.iter().map(|listener| {
        if listener.is_per_match() {
            quote!(#crate_name::MatchBy::PerMatch)
        } else {
            let capture_name = match listener.capture_name.as_ref() {
                Some(capture_name) => quote!(Some(#capture_name.into())),
                None => quote!(None),
            };
            quote! {
                #crate_name::MatchBy::PerCapture {
                    capture_name: #capture_name,
                }
            }
        }
    });
    let maybe_deserialize_options = match rule.options_type.as_ref() {
        None => quote!(),
        Some(options_type) => {
            if is_option_type(options_type) {
                quote! {
                    let options: #options_type = options.map(|options| {
                        #crate_name::serde_json::from_str(&options.to_string()).unwrap_or_else(|_| {
                            panic!("Couldn't parse rule options: {:#?}", options.to_string());
                        })
                    });
                }
            } else {
                quote! {
                    let options: #options_type = options.map(|options| {
                        #crate_name::serde_json::from_str(&options.to_string()).unwrap_or_else(|_| {
                            panic!("Couldn't parse rule options: {:#?}", options.to_string());
                        })
                    }).unwrap();
                }
            }
        }
    };
    let languages = &rule.languages;
    let messages = match rule.messages.as_ref() {
        Some(messages) => {
            let message_keys = messages.keys().map(|key| match key {
                Expr::Path(key) if key.path.get_ident().is_some() => quote!(stringify!(#key)),
                _ => quote!(#key),
            });
            let message_values = messages.values();
            quote! {
                Some([#((String::from(#message_keys), String::from(#message_values))),*].into())
            }
        }
        None => quote!(None),
    };
    quote! {
        impl #crate_name::Rule for #rule_struct_name {
            fn meta(&self) -> #crate_name::RuleMeta {
                #crate_name::RuleMeta {
                    name: #name.into(),
                    fixable: #fixable,
                    languages: vec![#(#crate_name::tree_sitter_grep::SupportedLanguage::#languages),*],
                    messages: #messages,
                }
            }

            fn instantiate(self: std::sync::Arc<Self>, _config: &#crate_name::Config, rule_configuration: &#crate_name::RuleConfiguration) -> std::sync::Arc<dyn #crate_name::RuleInstance> {
                let options = rule_configuration.options.as_ref();
                #maybe_deserialize_options
                std::sync::Arc::new(#rule_instance_struct_name {
                    rule: self.clone(),
                    listener_queries: vec![
                        #(#crate_name::RuleListenerQuery {
                            query: #rule_listener_queries.into(),
                            match_by: #rule_listener_match_bys,
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
    crate_name: &Ident,
) -> proc_macro2::TokenStream {
    let state_field_names = rule_instance_state_fields.iter().map(|field| &field.name);
    let state_field_types = rule_instance_state_fields.iter().map(|field| &field.type_);
    quote! {
        struct #rule_instance_struct_name {
            rule: std::sync::Arc<#rule_struct_name>,
            listener_queries: Vec<#crate_name::RuleListenerQuery>,
            #(#state_field_names: #state_field_types),*
        }
    }
}

fn get_rule_instance_rule_instance_impl(
    rule_instance_struct_name: &Ident,
    rule_instance_per_file_struct_name: &Ident,
    rule_instance_per_file_state_fields: &[&RuleStateFieldSpec],
    crate_name: &Ident,
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
        impl #crate_name::RuleInstance for #rule_instance_struct_name {
            fn instantiate_per_file(
                self: std::sync::Arc<Self>,
                _file_run_info: &#crate_name::FileRunInfo,
            ) -> Box<dyn #crate_name::RuleInstancePerFile> {
                Box::new(#rule_instance_per_file_struct_name {
                    rule_instance: self,
                    #(#rule_instance_per_file_state_field_names: #rule_instance_per_file_state_field_initializers),*
                })
            }

            fn rule(&self) -> std::sync::Arc<dyn #crate_name::Rule> {
                self.rule.clone()
            }

            fn listener_queries(&self) -> &[#crate_name::RuleListenerQuery] {
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

#[derive(Copy, Clone)]
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
                    *node = parse_quote!(self.rule_instance.#self_field_name);
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
            Ok(macro_args) => {
                let rewritten_macro_args = macro_args
                    .into_iter()
                    .map(|mut macro_arg| {
                        SelfAccessRewriter { rule: self.rule }.visit_expr_mut(&mut macro_arg);
                        macro_arg
                    })
                    .collect::<Vec<_>>();
                quote! {
                    #(#rewritten_macro_args),*
                }
            }
            _ => match syn::parse2::<
                ArrowSeparatedKeyValuePairs<Ident, ExprOrArrowSeparatedKeyValuePairs>,
            >(node.mac.tokens.clone())
            {
                Ok(mut arrow_separated_key_value_pairs) => {
                    rewrite_self_accesses_in_arrow_separated_key_value_pairs(
                        *self,
                        &mut arrow_separated_key_value_pairs,
                    );
                    let keys = arrow_separated_key_value_pairs.keys_and_values.keys();
                    let values = arrow_separated_key_value_pairs.keys_and_values.values();
                    quote! {
                        #(#keys => #values),*
                    }
                }
                _ => return,
            },
        };
        node.mac.tokens = rewritten_macro_args;
        visit_mut::visit_expr_macro_mut(self, node);
    }
}

fn rewrite_self_accesses_in_arrow_separated_key_value_pairs(
    mut rewriter: SelfAccessRewriter,
    arrow_separated_key_value_pairs: &mut ArrowSeparatedKeyValuePairs<
        Ident,
        ExprOrArrowSeparatedKeyValuePairs,
    >,
) {
    arrow_separated_key_value_pairs
        .keys_and_values
        .values_mut()
        .for_each(|value| match value {
            ExprOrArrowSeparatedKeyValuePairs::Expr(value) => {
                rewriter.visit_expr_mut(value);
            }
            ExprOrArrowSeparatedKeyValuePairs::ArrowSeparatedKeyValuePairs(value) => {
                rewrite_self_accesses_in_arrow_separated_key_value_pairs(rewriter, value);
            }
        });
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
    crate_name: &Ident,
) -> proc_macro2::TokenStream {
    let listener_indices = 0..rule.listeners.len();
    let listener_callbacks = rule.listeners.iter().map(|listener| {
        let mut callback_body = listener.callback.body.clone();

        SelfAccessRewriter { rule }.visit_expr_mut(&mut callback_body);

        let node_in_scope = if listener.is_per_match() {
            quote!()
        } else {
            quote! {
                let node = match node_or_captures {
                    #crate_name::NodeOrCaptures::Node(node) => node,
                    _ => panic!("Expected node"),
                };
            }
        };

        let captures_in_scope = if listener.is_per_match() {
            quote! {
                let captures = match node_or_captures {
                    #crate_name::NodeOrCaptures::Captures(captures) => captures,
                    _ => panic!("Expected captures"),
                };
            }
        } else {
            quote!()
        };

        quote! {
            #node_in_scope

            #captures_in_scope

            #callback_body
        }
    });
    quote! {
        impl #crate_name::RuleInstancePerFile for #rule_instance_per_file_struct_name {
            fn on_query_match(&mut self, listener_index: usize, node_or_captures: #crate_name::NodeOrCaptures, context: &mut #crate_name::QueryMatchContext) {
                match listener_index {
                    #(#listener_indices => {
                        #listener_callbacks
                    })*
                    _ => unreachable!(),
                }
            }

            fn rule_instance(&self) -> std::sync::Arc<dyn #crate_name::RuleInstance> {
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
        std::sync::Arc::new(#rule_struct_name {
            #(#rule_state_field_names: #rule_state_field_initializers),*
        })
    }
}
