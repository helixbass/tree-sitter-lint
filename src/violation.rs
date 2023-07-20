use derive_builder::Builder;
use tree_sitter::Node;

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'a> {
    pub message: String,
    pub node: Node<'a>,
}
