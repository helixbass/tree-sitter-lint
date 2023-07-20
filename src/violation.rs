use derive_builder::Builder;
use tree_sitter::Node;

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'node> {
    pub message: String,
    pub node: &'node Node<'node>,
}
