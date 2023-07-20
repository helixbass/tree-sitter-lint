use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, ExprPath, Ident, Token,
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
