use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, Ident, Token,
};

use crate::helpers::ExprOrArrowSeparatedKeyValuePairs;

struct Violation {
    message: Option<Expr>,
    message_id: Option<Expr>,
    node: Expr,
    range: Option<Expr>,
    fix: Option<Expr>,
    data: Option<ExprOrArrowSeparatedKeyValuePairs>,
}

impl Parse for Violation {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut message: Option<Expr> = Default::default();
        let mut message_id: Option<Expr> = Default::default();
        let mut node: Option<Expr> = Default::default();
        let mut range: Option<Expr> = Default::default();
        let mut fix: Option<Expr> = Default::default();
        let mut data: Option<ExprOrArrowSeparatedKeyValuePairs> = Default::default();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=>]>()?;
            match &*key.to_string() {
                "message" => {
                    assert!(message.is_none(), "Already saw 'message'");
                    message = Some(input.parse()?);
                }
                "message_id" => {
                    assert!(message_id.is_none(), "Already saw 'message_id'");
                    message_id = Some(input.parse()?);
                }
                "node" => {
                    assert!(node.is_none(), "Already saw 'node'");
                    node = Some(input.parse()?);
                }
                "range" => {
                    assert!(range.is_none(), "Already saw 'range'");
                    range = Some(input.parse()?);
                }
                "fix" => {
                    assert!(fix.is_none(), "Already saw 'fix'");
                    fix = Some(input.parse()?);
                }
                "data" => {
                    assert!(data.is_none(), "already saw 'data' key");
                    data = Some(input.parse()?);
                }
                _ => panic!("Unexpected key: '{key}'"),
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            message,
            message_id,
            node: node.expect("Expected 'node' key"),
            range,
            fix,
            data,
        })
    }
}

pub fn violation_with_crate_name(input: TokenStream, crate_name: &str) -> TokenStream {
    let violation: Violation = parse_macro_input!(input);

    let crate_name = format_ident!("{}", crate_name);

    let message = match violation.message.as_ref() {
        Some(message) => quote!(.message(#message)),
        None => quote!(),
    };

    let message_id = match violation.message_id.as_ref() {
        Some(message_id) => quote!(.message_id(#message_id)),
        None => quote!(),
    };

    let fix = match violation.fix.as_ref() {
        Some(fix) => quote!(.fix(#fix)),
        None => quote!(),
    };

    let data = match violation.data.as_ref() {
        Some(data) => {
            let data = match data {
                ExprOrArrowSeparatedKeyValuePairs::Expr(data) => quote!(#data),
                ExprOrArrowSeparatedKeyValuePairs::ArrowSeparatedKeyValuePairs(data) => {
                    let data_keys = data
                        .keys_and_values
                        .keys()
                        .map(|key| quote!(stringify!(#key)));
                    // ExprOrIdent::Ident(key) => quote!(stringify!(#key)),
                    // ExprOrIdent::Expr(Expr::Path(key)) if key.path.get_ident().is_some() => {
                    //     quote!(stringify!(#key))
                    // }
                    // _ => quote!(#key),
                    let data_values = data.keys_and_values.values();
                    quote! {
                        [#((#data_keys.to_string(), #data_values.to_string())),*]
                    }
                }
            };
            quote! {
                .data(#data)
            }
        }
        None => quote!(),
    };

    let node = &violation.node;

    let range = match violation.range.as_ref() {
        Some(range) => quote!(.range(#range)),
        None => quote!(),
    };

    quote! {
        #crate_name::ViolationBuilder::default()
            #message
            #message_id
            #fix
            .node(#node)
            #range
            #data
            .build().unwrap()
    }
    .into()
}
