use std::collections::HashMap;

use syn::{
    parse::{Parse, ParseStream},
    Expr, Ident, Token,
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
