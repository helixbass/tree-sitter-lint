use std::collections::HashMap;

use quote::ToTokens;
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    spanned::Spanned,
    token, Expr, Ident, Token,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ExprOrIdent {
    Expr(Expr),
    Ident(Ident),
}

impl Parse for ExprOrIdent {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input
            .parse::<Ident>()
            .map(Self::Ident)
            .or_else(|_| -> syn::Result<_> {
                let key = input.parse::<Token![type]>()?;
                Ok(Self::Ident(Ident::new("type_", key.span())))
            })
            .or_else(|_| Ok(Self::Expr(input.parse::<Expr>()?)))
    }
}

impl ToTokens for ExprOrIdent {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Expr(expr) => expr.to_tokens(tokens),
            Self::Ident(ident) => ident.to_tokens(tokens),
        }
    }
}

impl From<Expr> for ExprOrIdent {
    fn from(value: Expr) -> Self {
        Self::Expr(value)
    }
}

impl From<Ident> for ExprOrIdent {
    fn from(value: Ident) -> Self {
        Self::Ident(value)
    }
}

pub fn parse_data(data: &mut HashMap<ExprOrIdent, Expr>, input: ParseStream) -> syn::Result<()> {
    let data_content;
    if input.peek(token::Brace) {
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
            let value: Expr = data_content.parse()?;
            data.insert(key, value);
            if !data_content.is_empty() {
                data_content.parse::<Token![,]>()?;
            }
        }
    } else {
        bracketed!(data_content in input);
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
            let value: Expr = data_content.parse()?;
            data.insert(key, value);
            if !data_content.is_empty() {
                data_content.parse::<Token![,]>()?;
            }
        }
    }

    Ok(())
}
