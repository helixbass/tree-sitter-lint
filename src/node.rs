use std::{borrow::Cow, cmp::Ordering, collections::HashSet};

use squalid::{return_default_if_false, return_default_if_none, Contains, OptionExt};
use tree_sitter_grep::{
    tree_sitter::{Node, TreeCursor},
    SupportedLanguage,
};

use crate::{
    context::TokenWalker, get_tokens, rule_tester::compare_ranges, QueryMatchContext, SkipOptions,
    SkipOptionsBuilder, SourceTextProvider,
};

pub type Kind = &'static str;

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
    fn non_comment_children_and_field_names(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentChildrenAndFieldNames<'a>;
    fn is_only_non_comment_named_sibling(&self, language: impl Into<SupportedLanguage>) -> bool;
    fn has_trailing_comments(&self, context: &QueryMatchContext<'a, '_>) -> bool;
    fn maybe_first_non_comment_named_child(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> Option<Node<'a>>;
    fn first_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> Node<'a>;
    fn skip_nodes_of_types(
        &self,
        kinds: &[Kind],
        language: impl Into<SupportedLanguage>,
    ) -> Node<'a>;
    fn skip_nodes_of_type(&self, kind: Kind, language: impl Into<SupportedLanguage>) -> Node<'a>;
    fn ancestors(&self) -> Ancestors<'a>;
    fn next_ancestor_not_of_kinds(&self, kinds: &[Kind]) -> Node<'a>;
    fn next_ancestor_not_of_kind(&self, kind: Kind) -> Node<'a>;
    fn next_ancestor_of_kind(&self, kind: Kind) -> Node<'a>;
    fn has_child_of_kind(&self, kind: Kind) -> bool;
    fn maybe_first_child_of_kind(&self, kind: Kind) -> Option<Node<'a>>;
    fn has_non_comment_named_children(&self, language: impl Into<SupportedLanguage>) -> bool;
    fn when_kind(&self, kind: Kind) -> Option<Node<'a>>;
    fn is_first_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> bool;
    fn is_last_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> bool;
    fn num_non_comment_named_children(&self, language: impl Into<SupportedLanguage>) -> usize;
    fn first_non_comment_child(&self, language: impl Into<SupportedLanguage>) -> Node<'a>;
    fn has_child_of_kinds(&self, kinds: &impl Contains<Kind>) -> bool;
    fn maybe_first_child_of_kinds(&self, kinds: &impl Contains<Kind>) -> Option<Node<'a>>;
    fn tokens(&self) -> TokenWalker<'a>;
    fn non_comment_named_children_and_field_names(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentNamedChildrenAndFieldNames<'a>;
    fn children_of_kind(&self, kind: Kind) -> ChildrenOfKind<'a>;
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

    fn non_comment_children_and_field_names(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentChildrenAndFieldNames<'a> {
        let language = language.into();

        NonCommentChildrenAndFieldNames::new(*self, language.comment_kinds())
    }

    fn is_only_non_comment_named_sibling(&self, language: impl Into<SupportedLanguage>) -> bool {
        assert!(self.is_named());
        let parent = return_default_if_none!(self.parent());
        parent.non_comment_named_children(language).count() == 1
    }

    fn has_trailing_comments(&self, context: &QueryMatchContext<'a, '_>) -> bool {
        let language: SupportedLanguage = context.into();

        language.comment_kinds().contains(
            &context
                .get_last_token(
                    *self,
                    Option::<SkipOptions<fn(Node) -> bool>>::Some(
                        SkipOptionsBuilder::default()
                            .include_comments(true)
                            .build()
                            .unwrap(),
                    ),
                )
                .kind(),
        )
    }

    fn maybe_first_non_comment_named_child(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> Option<Node<'a>> {
        self.non_comment_named_children(language).next()
    }

    fn first_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> Node<'a> {
        self.non_comment_named_children(language).next().unwrap()
    }

    fn skip_nodes_of_types(
        &self,
        kinds: &[Kind],
        language: impl Into<SupportedLanguage>,
    ) -> Node<'a> {
        skip_nodes_of_types(*self, kinds, language)
    }

    fn skip_nodes_of_type(&self, kind: Kind, language: impl Into<SupportedLanguage>) -> Node<'a> {
        skip_nodes_of_type(*self, kind, language)
    }

    fn ancestors(&self) -> Ancestors<'a> {
        Ancestors::new(*self)
    }

    fn next_ancestor_not_of_kinds(&self, kinds: &[Kind]) -> Node<'a> {
        let mut node = self.parent().unwrap();
        while kinds.contains(&node.kind()) {
            node = node.parent().unwrap();
        }
        node
    }

    fn next_ancestor_not_of_kind(&self, kind: Kind) -> Node<'a> {
        let mut node = self.parent().unwrap();
        while node.kind() == kind {
            node = node.parent().unwrap();
        }
        node
    }

    fn next_ancestor_of_kind(&self, kind: Kind) -> Node<'a> {
        let mut node = self.parent().unwrap();
        while node.kind() != kind {
            node = node.parent().unwrap();
        }
        node
    }

    fn has_child_of_kind(&self, kind: Kind) -> bool {
        self.maybe_first_child_of_kind(kind).is_some()
    }

    fn maybe_first_child_of_kind(&self, kind: Kind) -> Option<Node<'a>> {
        let mut cursor = self.walk();
        let ret = self
            .children(&mut cursor)
            .find(|child| child.kind() == kind);
        ret
    }

    fn has_non_comment_named_children(&self, language: impl Into<SupportedLanguage>) -> bool {
        self.non_comment_named_children(language).count() > 0
    }

    fn when_kind(&self, kind: Kind) -> Option<Node<'a>> {
        (self.kind() == kind).then_some(*self)
    }

    fn is_first_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> bool {
        self.parent()
            .matches(|parent| parent.maybe_first_non_comment_named_child(language) == Some(*self))
    }

    fn is_last_non_comment_named_child(&self, language: impl Into<SupportedLanguage>) -> bool {
        let language = language.into();

        let mut current_node = *self;
        while let Some(next_sibling) = current_node.next_named_sibling() {
            if !language.comment_kinds().contains(&next_sibling.kind()) {
                return false;
            }
            current_node = next_sibling;
        }
        true
    }

    fn num_non_comment_named_children(&self, language: impl Into<SupportedLanguage>) -> usize {
        self.non_comment_named_children(language).count()
    }

    fn first_non_comment_child(&self, language: impl Into<SupportedLanguage>) -> Node<'a> {
        self.non_comment_children(language).next().unwrap()
    }

    fn has_child_of_kinds(&self, kinds: &impl Contains<Kind>) -> bool {
        self.maybe_first_child_of_kinds(kinds).is_some()
    }

    fn maybe_first_child_of_kinds(&self, kinds: &impl Contains<Kind>) -> Option<Node<'a>> {
        let mut cursor = self.walk();
        let ret = self
            .children(&mut cursor)
            .find(|child| kinds.contains_(&child.kind()));
        ret
    }

    fn tokens(&self) -> TokenWalker<'a> {
        get_tokens(*self)
    }

    fn non_comment_named_children_and_field_names(
        &self,
        language: impl Into<SupportedLanguage>,
    ) -> NonCommentNamedChildrenAndFieldNames<'a> {
        let language = language.into();

        NonCommentNamedChildrenAndFieldNames::new(*self, language.comment_kinds())
    }

    fn children_of_kind(&self, kind: Kind) -> ChildrenOfKind<'a> {
        ChildrenOfKind::new(*self, kind)
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

pub struct NonCommentChildrenAndFieldNames<'a> {
    cursor: TreeCursor<'a>,
    is_done: bool,
    comment_kinds: &'static HashSet<&'static str>,
}

impl<'a> NonCommentChildrenAndFieldNames<'a> {
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

impl<'a> Iterator for NonCommentChildrenAndFieldNames<'a> {
    type Item = (Node<'a>, Option<&'static str>);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.is_done {
            let node = self.cursor.node();
            let field_name = self.cursor.field_name();
            self.is_done = !self.cursor.goto_next_sibling();
            if !self.comment_kinds.contains(&node.kind()) {
                return Some((node, field_name));
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

pub struct NonCommentNamedChildrenAndFieldNames<'a> {
    cursor: TreeCursor<'a>,
    is_done: bool,
    comment_kinds: &'static HashSet<&'static str>,
}

impl<'a> NonCommentNamedChildrenAndFieldNames<'a> {
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

impl<'a> Iterator for NonCommentNamedChildrenAndFieldNames<'a> {
    type Item = (Node<'a>, Option<&'static str>);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.is_done {
            let node = self.cursor.node();
            let field_name = self.cursor.field_name();
            self.is_done = !self.cursor.goto_next_sibling();
            if node.is_named() && !self.comment_kinds.contains(&node.kind()) {
                return Some((node, field_name));
            }
        }
        None
    }
}

pub struct ChildrenOfKind<'a> {
    cursor: TreeCursor<'a>,
    is_done: bool,
    kind: Kind,
}

impl<'a> ChildrenOfKind<'a> {
    pub fn new(node: Node<'a>, kind: Kind) -> Self {
        let mut cursor = node.walk();
        let is_done = !cursor.goto_first_child();
        Self {
            cursor,
            is_done,
            kind,
        }
    }
}

impl<'a> Iterator for ChildrenOfKind<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.is_done {
            let node = self.cursor.node();
            self.is_done = !self.cursor.goto_next_sibling();
            if node.kind() == self.kind {
                return Some(node);
            }
        }
        None
    }
}

pub struct Ancestors<'a> {
    current_node: Option<Node<'a>>,
}

impl<'a> Ancestors<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            current_node: node.parent(),
        }
    }
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.current_node;
        self.current_node = self
            .current_node
            .and_then(|current_node| current_node.parent());
        ret
    }
}

pub fn skip_nodes_of_type(
    mut node: Node,
    kind: Kind,
    language: impl Into<SupportedLanguage>,
) -> Node {
    let language = language.into();
    let comment_kinds = language.comment_kinds();

    while node.kind() == kind {
        let mut cursor = node.walk();
        if !cursor.goto_first_child() {
            return node;
        }
        while comment_kinds.contains(&cursor.node().kind()) || !cursor.node().is_named() {
            if !cursor.goto_next_sibling() {
                return node;
            }
        }
        node = cursor.node();
    }
    node
}

pub fn skip_nodes_of_types<'a>(
    mut node: Node<'a>,
    kinds: &[Kind],
    language: impl Into<SupportedLanguage>,
) -> Node<'a> {
    let language = language.into();
    let comment_kinds = language.comment_kinds();

    while kinds.contains(&node.kind()) {
        let mut cursor = node.walk();
        if !cursor.goto_first_child() {
            return node;
        }
        while comment_kinds.contains(&cursor.node().kind()) || !cursor.node().is_named() {
            if !cursor.goto_next_sibling() {
                return node;
            }
        }
        node = cursor.node();
    }
    node
}
