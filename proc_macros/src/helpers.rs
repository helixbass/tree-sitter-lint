use std::collections::HashMap;

use quote::{quote, ToTokens};
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    token, Expr, Ident, Token,
};

pub struct ArrowSeparatedKeyValuePair<TKey = Ident, TValue = Expr> {
    key: TKey,
    value: TValue,
}

impl<TKey, TValue> Parse for ArrowSeparatedKeyValuePair<TKey, TValue>
where
    TKey: Parse,
    TValue: Parse,
{
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: TKey = input.parse()?;
        input.parse::<Token![=>]>()?;
        let value: TValue = input.parse()?;
        Ok(Self { key, value })
    }
}

pub struct ArrowSeparatedKeyValuePairs<TKey = Ident, TValue = Expr> {
    pub keys_and_values: HashMap<TKey, TValue>,
}

impl<TKey, TValue> Parse for ArrowSeparatedKeyValuePairs<TKey, TValue>
where
    TKey: Parse + Eq + std::hash::Hash,
    TValue: Parse,
{
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut keys_and_values: HashMap<TKey, TValue> = Default::default();
        while !input.is_empty() {
            let ArrowSeparatedKeyValuePair { key, value } = input.parse()?;
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
            keys_and_values.insert(key, value);
        }
        Ok(Self { keys_and_values })
    }
}

impl<TKey, TValue> ToTokens for ArrowSeparatedKeyValuePairs<TKey, TValue>
where
    TKey: ToTokens + Eq + std::hash::Hash,
    TValue: ToTokens,
{
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let keys = self.keys_and_values.keys();
        let values = self.keys_and_values.values();
        quote! {
            {
                #(#keys => #values),*
            }
        }
        .to_tokens(tokens)
    }
}

pub enum ExprOrArrowSeparatedKeyValuePairs {
    Expr(Expr),
    ArrowSeparatedKeyValuePairs(
        ArrowSeparatedKeyValuePairs<Ident, ExprOrArrowSeparatedKeyValuePairs>,
    ),
}

impl Parse for ExprOrArrowSeparatedKeyValuePairs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(token::Brace) {
            let braced_input;
            braced!(braced_input in input);
            let arrow_separated_key_value_pairs = braced_input
                .parse::<ArrowSeparatedKeyValuePairs<Ident, ExprOrArrowSeparatedKeyValuePairs>>();
            if let Ok(arrow_separated_key_value_pairs) = arrow_separated_key_value_pairs {
                return Ok(Self::ArrowSeparatedKeyValuePairs(
                    arrow_separated_key_value_pairs,
                ));
            };
        }
        if input.peek(token::Bracket) {
            let bracketed_input;
            bracketed!(bracketed_input in input);
            let arrow_separated_key_value_pairs = bracketed_input
                .parse::<ArrowSeparatedKeyValuePairs<Ident, ExprOrArrowSeparatedKeyValuePairs>>();
            if let Ok(arrow_separated_key_value_pairs) = arrow_separated_key_value_pairs {
                return Ok(Self::ArrowSeparatedKeyValuePairs(
                    arrow_separated_key_value_pairs,
                ));
            };
        }
        input.parse::<Expr>().map(Self::Expr)
    }
}

impl ToTokens for ExprOrArrowSeparatedKeyValuePairs {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Expr(expr) => expr.to_tokens(tokens),
            Self::ArrowSeparatedKeyValuePairs(arrow_separated_key_value_pairs) => {
                arrow_separated_key_value_pairs.to_tokens(tokens)
            }
        }
    }
}
