use squalid::return_default_if_false;
use tree_sitter_grep::tree_sitter::{Node, TreeCursor};

pub trait NodeExt<'a> {
    fn is_descendant_of(&self, node: Node) -> bool;
    fn field(&self, field_name: &str) -> Node<'a>;
    fn root(&self) -> Node<'a>;
    fn get_cursor_scoped_to_root(&self) -> TreeCursor<'a>;
    fn find_first_matching_descendant(
        &self,
        predicate: impl FnMut(Node) -> bool,
    ) -> Option<Node<'a>>;
    fn find_first_descendant_of_kind(&self, kind: &str) -> Option<Node<'a>>;
}

impl<'a> NodeExt<'a> for Node<'a> {
    fn is_descendant_of(&self, node: Node) -> bool {
        self.range().start_byte >= node.range().start_byte
            && self.range().end_byte <= node.range().end_byte
    }

    fn field(&self, field_name: &str) -> Node<'a> {
        self.child_by_field_name(field_name).unwrap_or_else(|| {
            panic!("Expected field '{field_name}'");
        })
    }

    fn root(&self) -> Node<'a> {
        let mut node = *self;
        while let Some(parent) = node.parent() {
            node = parent;
        }
        node
    }

    fn get_cursor_scoped_to_root(&self) -> TreeCursor<'a> {
        let mut cursor = self.root().walk();
        walk_cursor_to_descendant(&mut cursor, *self);
        cursor
    }

    fn find_first_matching_descendant(
        &self,
        mut predicate: impl FnMut(Node) -> bool,
    ) -> Option<Node<'a>> {
        if predicate(*self) {
            return Some(*self);
        }
        let mut cursor = self.walk();
        return_default_if_false!(cursor.goto_first_child());
        'outer: while cursor.node() != *self {
            while cursor.goto_first_child() {}
            if predicate(cursor.node()) {
                return Some(cursor.node());
            }
            loop {
                if cursor.goto_next_sibling() {
                    continue 'outer;
                }
                assert!(cursor.goto_parent());
                if cursor.node() == *self {
                    break 'outer;
                }
            }
        }
        None
    }

    fn find_first_descendant_of_kind(&self, kind: &str) -> Option<Node<'a>> {
        self.find_first_matching_descendant(|node| node.kind() == kind)
    }
}

fn walk_cursor_to_descendant(cursor: &mut TreeCursor, node: Node) {
    while cursor.node() != node {
        // this seems like it should be right but see https://github.com/tree-sitter/tree-sitter/issues/2463
        // cursor.goto_first_child_for_byte(node.start_byte()).unwrap();
        if node.is_descendant_of(cursor.node()) {
            assert!(cursor.goto_first_child());
        } else {
            assert!(cursor.goto_next_sibling());
        }
    }
}
