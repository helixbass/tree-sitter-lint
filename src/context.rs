use std::{ops, path::Path};

use tree_sitter_grep::{
    streaming_iterator::StreamingIterator, tree_sitter::TreeCursor, RopeOrSlice, SupportedLanguage,
};

use crate::{
    rule::InstantiatedRule,
    tree_sitter::{Language, Node, Query},
    violation::{Violation, ViolationWithContext},
    Config,
};

pub struct QueryMatchContext<'a> {
    pub path: &'a Path,
    pub file_contents: RopeOrSlice<'a>,
    pub rule: &'a InstantiatedRule,
    config: &'a Config,
    pub language: SupportedLanguage,
    pending_fixes: Option<Vec<PendingFix>>,
    pub violations: Option<Vec<ViolationWithContext>>,
}

impl<'a> QueryMatchContext<'a> {
    pub fn new(
        path: &'a Path,
        file_contents: impl Into<RopeOrSlice<'a>>,
        rule: &'a InstantiatedRule,
        config: &'a Config,
        language: SupportedLanguage,
    ) -> Self {
        let file_contents = file_contents.into();
        Self {
            path,
            file_contents,
            rule,
            config,
            language,
            pending_fixes: Default::default(),
            violations: Default::default(),
        }
    }

    pub fn report(&mut self, violation: Violation) {
        if self.config.fix {
            if let Some(fix) = violation.fix.as_ref() {
                if !self.rule.meta.fixable {
                    panic!("Rule {:?} isn't declared as fixable", self.rule.meta.name);
                }
                let mut fixer = Fixer::default();
                fix(&mut fixer);
                if let Some(pending_fixes) = fixer.into_pending_fixes() {
                    self.pending_fixes
                        .get_or_insert_with(Default::default)
                        .extend(pending_fixes);
                }
                if !self.config.report_fixed_violations {
                    return;
                }
            }
        }
        let violation = violation.contextualize(self);
        self.violations
            .get_or_insert_with(Default::default)
            .push(violation);
    }

    pub fn get_node_text(&self, node: Node) -> &'a str {
        get_node_text(node, self.file_contents)
    }

    pub fn maybe_get_single_captured_node_for_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        self.maybe_get_single_captured_node_for_filtered_query(query, |_| true, enclosing_node)
    }

    pub fn maybe_get_single_captured_node_for_filtered_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        mut predicate: impl FnMut(Node) -> bool,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        let query = query.into().into_parsed(self.language.language());
        let captures = tree_sitter_grep::get_captures_for_enclosing_node(
            self.file_contents,
            &query,
            0,
            None,
            enclosing_node,
        );
        let mut filtered_captures = captures
            .filter_map(|capture_info| predicate(capture_info.node).then_some(capture_info.node));
        let first_node = *filtered_captures.next()?;
        if filtered_captures.next().is_some() {
            return None;
        }
        Some(first_node)
    }

    pub fn get_number_of_query_captures<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> usize {
        self.get_number_of_filtered_query_captures(query, |_| true, enclosing_node)
    }

    pub fn get_number_of_filtered_query_captures<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        mut predicate: impl FnMut(Node) -> bool,
        enclosing_node: Node<'enclosing_node>,
    ) -> usize {
        let query = query.into().into_parsed(self.language.language());
        tree_sitter_grep::get_captures_for_enclosing_node(
            self.file_contents,
            &query,
            0,
            None,
            enclosing_node,
        )
        .filter(|capture_info| predicate(capture_info.node))
        .count()
    }

    pub fn pending_fixes(&self) -> Option<&[PendingFix]> {
        self.pending_fixes.as_deref()
    }

    pub fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes
    }

    pub fn has_named_child_of_kind(&self, node: Node, kind: &str) -> bool {
        let mut cursor = node.walk();
        let ret = node
            .named_children(&mut cursor)
            .any(|child| child.kind() == kind);
        ret
    }

    pub fn get_tokens(&self, node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
        get_tokens(node)
    }
}

pub enum ParsedOrUnparsedQuery<'a> {
    Parsed(Query),
    ParsedRef(&'a Query),
    Unparsed(&'a str),
}

impl<'a> ParsedOrUnparsedQuery<'a> {
    pub fn parsed(&self, language: Language) -> MaybeOwned<'_, Query> {
        match self {
            Self::Parsed(query) => query.into(),
            Self::ParsedRef(query) => (*query).into(),
            Self::Unparsed(query_text) => Query::new(language, query_text).unwrap().into(),
        }
    }

    pub fn into_parsed(self, language: Language) -> MaybeOwned<'a, Query> {
        match self {
            Self::Parsed(query) => query.into(),
            Self::ParsedRef(query) => query.into(),
            Self::Unparsed(query_text) => Query::new(language, query_text).unwrap().into(),
        }
    }
}

impl<'a> From<Query> for ParsedOrUnparsedQuery<'a> {
    fn from(value: Query) -> Self {
        Self::Parsed(value)
    }
}

impl<'a> From<&'a Query> for ParsedOrUnparsedQuery<'a> {
    fn from(value: &'a Query) -> Self {
        Self::ParsedRef(value)
    }
}

impl<'a> From<&'a str> for ParsedOrUnparsedQuery<'a> {
    fn from(value: &'a str) -> Self {
        Self::Unparsed(value)
    }
}

pub enum MaybeOwned<'a, T> {
    Owned(T),
    Borrowed(&'a T),
}

impl<'a, T> ops::Deref for MaybeOwned<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            MaybeOwned::Owned(value) => value,
            MaybeOwned::Borrowed(value) => value,
        }
    }
}

impl<'a, T> From<T> for MaybeOwned<'a, T> {
    fn from(value: T) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> From<&'a T> for MaybeOwned<'a, T> {
    fn from(value: &'a T) -> Self {
        Self::Borrowed(value)
    }
}

#[derive(Default)]
pub struct Fixer {
    pending_fixes: Option<Vec<PendingFix>>,
}

impl Fixer {
    pub fn replace_text(&mut self, node: Node, replacement: impl Into<String>) {
        self.pending_fixes
            .get_or_insert_with(Default::default)
            .push(PendingFix::new(node.byte_range(), replacement.into()));
    }

    pub fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes
    }
}

#[derive(Clone)]
pub struct PendingFix {
    pub range: ops::Range<usize>,
    pub replacement: String,
}

impl PendingFix {
    pub fn new(range: ops::Range<usize>, replacement: String) -> Self {
        Self { range, replacement }
    }
}

fn get_node_text<'a>(node: Node, file_contents: RopeOrSlice<'a>) -> &'a str {
    match file_contents {
        RopeOrSlice::Slice(file_contents) => node.utf8_text(file_contents).unwrap(),
        RopeOrSlice::Rope(_) => unimplemented!(),
    }
}

macro_rules! move_to_next_sibling_or_go_to_parent_and_loop {
    ($self:expr) => {
        if !$self.cursor.goto_next_sibling() {
            $self.cursor.goto_parent();
            $self.state = JustReturnedToParent;
            continue;
        }
    };
}

macro_rules! loop_if_on_comment {
    ($self:expr) => {
        if $self.cursor.node().kind() == "comment" {
            $self.state = OnComment;
            continue;
        }
    };
}

macro_rules! loop_landed_on_node {
    ($self:expr) => {
        $self.state = LandedOnNonCommentNode;
        continue;
    };
}

macro_rules! loop_done {
    ($self:expr) => {
        $self.state = Done;
        continue;
    };
}

fn get_tokens(node: Node) -> impl Iterator<Item = Node> {
    TokenWalker::new(node)
}

struct TokenWalker<'a> {
    state: TokenWalkerState,
    cursor: TreeCursor<'a>,
    original_node: Node<'a>,
}

impl<'a> TokenWalker<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::Initial,
            cursor: node.walk(),
            original_node: node,
        }
    }
}

impl<'a> Iterator for TokenWalker<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use TokenWalkerState::*;

        loop {
            match self.state {
                Done => {
                    return None;
                }
                Initial => {
                    if self.cursor.node().kind() == "comment" {
                        loop_done!(self);
                    }
                    if !self.cursor.goto_first_child() {
                        self.state = Done;
                        return Some(self.cursor.node());
                    }
                    loop_landed_on_node!(self);
                }
                ReturnedCurrentNode => {
                    move_to_next_sibling_or_go_to_parent_and_loop!(self);
                    loop_if_on_comment!(self);
                    loop_landed_on_node!(self);
                }
                OnComment => {
                    move_to_next_sibling_or_go_to_parent_and_loop!(self);
                    loop_if_on_comment!(self);
                    loop_landed_on_node!(self);
                }
                LandedOnNonCommentNode => {
                    if !self.cursor.goto_first_child() {
                        self.state = ReturnedCurrentNode;
                        return Some(self.cursor.node());
                    }
                    loop_if_on_comment!(self);
                    loop_landed_on_node!(self);
                }
                JustReturnedToParent => {
                    if self.cursor.node() == self.original_node {
                        loop_done!(self);
                    }
                    move_to_next_sibling_or_go_to_parent_and_loop!(self);
                    loop_if_on_comment!(self);
                    loop_landed_on_node!(self);
                }
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TokenWalkerState {
    Initial,
    OnComment,
    ReturnedCurrentNode,
    JustReturnedToParent,
    LandedOnNonCommentNode,
    Done,
}

#[cfg(test)]
mod tests {
    use tree_sitter_grep::tree_sitter::Parser;

    use super::*;

    fn test_all_tokens_text(text: &str, all_tokens_text: &[&str]) {
        let mut parser = Parser::new();
        parser
            .set_language(SupportedLanguage::Javascript.language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            get_tokens(tree.root_node())
                .map(|node| node.utf8_text(text.as_bytes()).unwrap())
                .collect::<Vec<_>>(),
            all_tokens_text
        );
    }

    #[test]
    fn test_get_tokens_simple() {
        test_all_tokens_text("const x = 5;", &["const", "x", "=", "5", ";"]);
    }

    #[test]
    fn test_get_tokens_structured() {
        test_all_tokens_text(
            r#"
            const whee = function(foo) {
                for (let x = 1; x < 100; x++) {
                    foo(x);
                }
            }
        "#,
            &[
                "const", "whee", "=", "function", "(", "foo", ")", "{", "for", "(", "let", "x",
                "=", "1", ";", "x", "<", "100", ";", "x", "++", ")", "{", "foo", "(", "x", ")",
                ";", "}", "}",
            ],
        );
    }
}
