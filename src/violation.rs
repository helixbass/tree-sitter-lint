use derive_builder::Builder;
use tree_sitter::Node;

use crate::context::QueryMatchContext;

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'a> {
    pub message: String,
    pub node: &'a Node<'a>,
    pub query_match_context: &'a QueryMatchContext<'a>,
}
