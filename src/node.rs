use std::{borrow::Cow, cmp::Ordering, collections::HashSet};

use squalid::{return_default_if_false, return_default_if_none};
use tree_sitter_grep::{
    tree_sitter::{Node, TreeCursor},
    SupportedLanguage,
};

use crate::{rule_tester::compare_ranges, SourceTextProvider};

pub trait NodeExt<'a> {
    fn is_descendant_of(&self, node: Node) -> bool;
    fn is_same_or_descendant_of(&self, node: Node) -> bool;
    fn field(&self, field_name: &str) -> Node<'a>;
    fn root(&self) -> Node<'a>;
    fn get_cursor_scoped_to_root(&self) -> TreeCursor<'a>;
    fn find_first_matching_descendant(
        &self,
        predicate: impl FnMut(Node) -> bool,
    ) -> Option<Node<'a>>;
    fn find_first_descendant_of_kind(&self, kind: &str) -> Option<Node<'a>>;
    fn get_first_child_of_kind(&self, kind: &str) -> Node<'a>;
    fn next_named_sibling_of_kinds(&self, kinds: &[&str]) -> Node<'a>;
    fn non_comment_children(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentChildren<'a>;
    fn non_comment_named_children(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentNamedChildren<'a>;
    fn text<'b>(&self, source_text_provider: &impl SourceTextProvider<'b>) -> Cow<'b, str>;
}

impl<'a> NodeExt<'a> for Node<'a> {
    fn is_descendant_of(&self, node: Node) -> bool {
        if self.start_byte() < node.start_byte() {
            return false;
        }
        if self.end_byte() > node.end_byte() {
            return false;
        }
        if self.start_byte() == node.start_byte() && self.end_byte() == node.end_byte() {
            let mut ancestor = return_default_if_none!(self.parent());
            loop {
                if ancestor == node {
                    return true;
                }
                ancestor = return_default_if_none!(ancestor.parent());
            }
        }
        true
    }

    fn is_same_or_descendant_of(&self, node: Node) -> bool {
        *self == node || self.is_descendant_of(node)
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

    fn get_first_child_of_kind(&self, kind: &str) -> Node<'a> {
        let mut cursor = self.walk();
        let ret = self
            .children(&mut cursor)
            .find(|child| child.kind() == kind)
            .unwrap();
        ret
    }

    fn next_named_sibling_of_kinds(&self, kinds: &[&str]) -> Node<'a> {
        let mut current_node = *self;
        loop {
            current_node = current_node.next_named_sibling().unwrap();
            if kinds.contains(&current_node.kind()) {
                return current_node;
            }
        }
    }

    fn non_comment_children(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentChildren<'a> {
        let language = language.into();

        NonCommentChildren::new(*self, language.comment_kinds())
    }

    fn non_comment_named_children(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentNamedChildren<'a> {
        let language = language.into();

        NonCommentNamedChildren::new(*self, language.comment_kinds())
    }

    fn text<'b>(&self, source_text_provider: &impl SourceTextProvider<'b>) -> Cow<'b, str> {
        source_text_provider.node_text(*self)
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

pub fn compare_nodes(a: &Node, b: &Node) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    match compare_ranges(a.range(), b.range()) {
        Ordering::Less => Ordering::Less,
        Ordering::Greater => Ordering::Greater,
        Ordering::Equal => {
            if a.is_descendant_of(*b) {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
    }
}

pub struct NonCommentChildren<'a> {
    cursor: TreeCursor<'a>,
    is_done: bool,
    comment_kinds: &'static HashSet<&'static str>,
}

impl<'a> NonCommentChildren<'a> {
    pub fn new(node: Node<'a>, comment_kinds: &'static HashSet<&'static str>) -> Self {
        let mut cursor = node.walk();
        let is_done = !cursor.goto_first_child();
        Self {
            cursor,
            is_done,
            comment_kinds,
        }
    }
}

impl<'a> Iterator for NonCommentChildren<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.is_done {
            let node = self.cursor.node();
            self.is_done = !self.cursor.goto_next_sibling();
            if !self.comment_kinds.contains(&node.kind()) {
                return Some(node);
            }
        }
        None
    }
}

pub struct NonCommentNamedChildren<'a> {
    cursor: TreeCursor<'a>,
    is_done: bool,
    comment_kinds: &'static HashSet<&'static str>,
}

impl<'a> NonCommentNamedChildren<'a> {
    pub fn new(node: Node<'a>, comment_kinds: &'static HashSet<&'static str>) -> Self {
        let mut cursor = node.walk();
        let is_done = !cursor.goto_first_child();
        Self {
            cursor,
            is_done,
            comment_kinds,
        }
    }
}

impl<'a> Iterator for NonCommentNamedChildren<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.is_done {
            let node = self.cursor.node();
            self.is_done = !self.cursor.goto_next_sibling();
            if node.is_named() && !self.comment_kinds.contains(&node.kind()) {
                return Some(node);
            }
        }
        None
    }
}
