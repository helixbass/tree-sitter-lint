use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    braced,
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, Ident, Token,
};

struct Violation {
    message: Option<Expr>,
    message_id: Option<Expr>,
    node: Expr,
    fix: Option<Expr>,
    data: Option<HashMap<Expr, Expr>>,
}

impl Parse for Violation {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut message: Option<Expr> = Default::default();
        let mut message_id: Option<Expr> = Default::default();
        let mut node: Option<Expr> = Default::default();
        let mut fix: Option<Expr> = Default::default();
        let mut data: Option<HashMap<Expr, Expr>> = Default::default();

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
                "fix" => {
                    assert!(fix.is_none(), "Already saw 'fix'");
                    fix = Some(input.parse()?);
                }
                "data" => {
                    assert!(data.is_none(), "Already saw 'data'");
                    let data = data.get_or_insert_with(Default::default);
                    let data_content;
                    braced!(data_content in input);
                    while !data_content.is_empty() {
                        let key: Expr = data_content.parse()?;
                        data_content.parse::<Token![=>]>()?;
                        let value: Expr = data_content.parse()?;
                        data.insert(key, value);
                        if !data_content.is_empty() {
                            data_content.parse::<Token![,]>()?;
                        }
                    }
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
            let data_keys = data.keys().map(|key| match key {
                Expr::Path(key) if key.path.get_ident().is_some() => quote!(stringify!(#key)),
                _ => quote!(#key),
            });
            let data_values = data.values();
            quote! {
                .data([#((String::from(#data_keys), String::from(#data_values))),*])
            }
        }
        None => quote!(),
    };

    let node = &violation.node;

    quote! {
        #crate_name::ViolationBuilder::default()
            #message
            #message_id
            #fix
            .node(#node)
            #data
            .build().unwrap()
    }
    .into()
}
