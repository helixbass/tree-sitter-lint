use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    ops,
    path::Path,
};

use derive_builder::Builder;
use squalid::{IsEmpty, OptionExt};
use tree_sitter_grep::{streaming_iterator::StreamingIterator, RopeOrSlice, SupportedLanguage};

mod backward_tokens;
mod get_tokens;

use get_tokens::get_tokens;

use self::{backward_tokens::get_backward_tokens, get_tokens::get_tokens_after_node};
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
    pending_fixes: RefCell<Option<Vec<PendingFix>>>,
    pub violations: RefCell<Option<Vec<ViolationWithContext>>>,
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

    pub fn report(&self, violation: Violation) {
        let mut had_fixes = false;
        if self.config.fix {
            if let Some(fix) = violation.fix.as_ref() {
                if !self.rule.meta.fixable {
                    panic!("Rule {:?} isn't declared as fixable", self.rule.meta.name);
                }
                let mut fixer = Fixer::default();
                fix(&mut fixer);
                if let Some(pending_fixes) = fixer.into_pending_fixes() {
                    had_fixes = true;
                    self.pending_fixes
                        .borrow_mut()
                        .get_or_insert_with(Default::default)
                        .extend(pending_fixes);
                }
                if !self.config.report_fixed_violations {
                    return;
                }
            }
        }
        let violation = violation.contextualize(self, had_fixes);
        self.violations
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .push(violation);
    }

    pub fn get_node_text(&self, node: Node) -> Cow<'a, str> {
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

    pub fn pending_fixes(&self) -> Ref<Option<Vec<PendingFix>>> {
        self.pending_fixes.borrow()
    }

    pub fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes.into_inner()
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

    pub fn get_text_slice(&self, range: ops::Range<usize>) -> Cow<'a, str> {
        get_text_slice(self.file_contents, range)
    }

    pub fn maybe_get_token_after<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Option<Node<'a>> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_tokens_after_node(node)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self.language.comment_kinds().contains(node.kind())
                    }
            })
    }

    pub fn get_token_after<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        self.maybe_get_token_after(node, skip_options).unwrap()
    }

    pub fn get_last_token<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_backward_tokens(node)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self.language.comment_kinds().contains(node.kind())
                    }
            })
            .unwrap()
    }
}

#[derive(Builder)]
#[builder(default, setter(strip_option))]
pub struct SkipOptions<TFilter: FnMut(Node) -> bool> {
    skip: Option<usize>,
    include_comments: Option<bool>,
    filter: Option<TFilter>,
}

impl<TFilter: FnMut(Node) -> bool> SkipOptions<TFilter> {
    pub fn skip(&self) -> usize {
        self.skip.unwrap_or_default()
    }

    pub fn include_comments(&self) -> bool {
        self.include_comments.unwrap_or_default()
    }

    pub fn filter(&mut self) -> Option<&mut TFilter> {
        self.filter.as_mut()
    }
}

impl<TFilter: FnMut(Node) -> bool> Default for SkipOptions<TFilter> {
    fn default() -> Self {
        Self {
            skip: Default::default(),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl From<usize> for SkipOptions<fn(Node) -> bool> {
    fn from(value: usize) -> Self {
        Self {
            skip: Some(value),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl<TFilter: FnMut(Node) -> bool> From<TFilter> for SkipOptions<TFilter> {
    fn from(value: TFilter) -> Self {
        Self {
            skip: Default::default(),
            include_comments: Default::default(),
            filter: Some(value),
        }
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

    pub(crate) fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending_fixes
            .as_ref()
            .is_none_or_matches(|pending_fixes| pending_fixes.is_empty())
    }
}

impl IsEmpty for Fixer {
    fn _is_empty(&self) -> bool {
        self.is_empty()
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

fn get_node_text<'a>(node: Node, file_contents: RopeOrSlice<'a>) -> Cow<'a, str> {
    get_text_slice(file_contents, node.byte_range())
}

fn get_text_slice(file_contents: RopeOrSlice, range: ops::Range<usize>) -> Cow<'_, str> {
    match file_contents {
        RopeOrSlice::Slice(slice) => std::str::from_utf8(&slice[range]).unwrap().into(),
        RopeOrSlice::Rope(rope) => rope.byte_slice(range).into(),
    }
}