use squalid::OptionExt;
use tree_sitter_grep::tree_sitter::{Node, Tree};

pub trait TreeEnterLeaveVisitor<'a> {
    fn enter_node(&mut self, node: Node<'a>);
    fn leave_node(&mut self, node: Node<'a>);
}

pub fn walk_tree<'a>(tree: &'a Tree, visitor: &mut impl TreeEnterLeaveVisitor<'a>) {
    let mut node_stack: Vec<Node<'a>> = Default::default();
    let mut cursor = tree.walk();
    'outer: loop {
        let node = cursor.node();
        while node_stack
            .last()
            .matches(|&last| node.end_byte() > last.end_byte())
        {
            // trace!(target: "visit", ?node, "leaving node");

            visitor.leave_node(node_stack.pop().unwrap());
        }
        // trace!(target: "visit", ?node, "entering node");

        node_stack.push(node);
        visitor.enter_node(node);

        #[allow(clippy::collapsible_if)]
        if !cursor.goto_first_child() {
            if !cursor.goto_next_sibling() {
                while cursor.goto_parent() {
                    if cursor.goto_next_sibling() {
                        continue 'outer;
                    }
                }
                break;
            }
        }
    }
    while let Some(node) = node_stack.pop() {
        // trace!(target: "visit", ?node, "leaving node");

        visitor.leave_node(node);
    }
}
