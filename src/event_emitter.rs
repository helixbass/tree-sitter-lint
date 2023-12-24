use tree_sitter_grep::{tree_sitter::Node, SupportedLanguage};

pub type Event = String;

pub trait EventEmitter<'a> {
    fn name(&self) -> String;
    fn languages(&self) -> Vec<SupportedLanguage>;
    fn enter_node(&mut self, node: Node<'a>) -> Option<Vec<Event>>;
    fn leave_node(&mut self, node: Node<'a>) -> Option<Vec<Event>>;
}
