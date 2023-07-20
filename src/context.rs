use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use tree_sitter::{Language, Node, Query, QueryCursor};

use crate::{rule::ResolvedRule, violation::Violation};

pub struct Context {
    pub language: Language,
}

impl Context {
    pub fn new(language: Language) -> Self {
        Self { language }
    }
}

pub struct QueryMatchContext<'a> {
    pub path: &'a Path,
    pub file_contents: &'a [u8],
    pub rule: &'a ResolvedRule<'a>,
    reported_any_violations: &'a AtomicBool,
    context: &'a Context,
}

impl<'a> QueryMatchContext<'a> {
    pub fn new(
        path: &'a Path,
        file_contents: &'a [u8],
        rule: &'a ResolvedRule,
        reported_any_violations: &'a AtomicBool,
        context: &'a Context,
    ) -> Self {
        Self {
            path,
            file_contents,
            rule,
            reported_any_violations,
            context,
        }
    }

    pub fn report(&self, violation: Violation) {
        self.reported_any_violations.store(true, Ordering::Relaxed);
        print_violation(&violation, self);
    }

    pub fn get_node_text(&self, node: Node) -> &str {
        node.utf8_text(self.file_contents).unwrap()
    }

    pub fn maybe_get_single_matching_node_for_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        let query = query.into().into_parsed(self.context.language);
        let mut query_cursor = QueryCursor::new();
        let mut matches = query_cursor.matches(&query, enclosing_node, self.file_contents);
        let first_match = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let mut nodes_for_default_capture_index = first_match.nodes_for_capture_index(0);
        let first_node = nodes_for_default_capture_index.next()?;
        if nodes_for_default_capture_index.next().is_some() {
            return None;
        }
        Some(first_node)
    }
}

pub enum ParsedOrUnparsedQuery<'query_text> {
    Parsed(Query),
    Unparsed(&'query_text str),
}

impl<'query_text> ParsedOrUnparsedQuery<'query_text> {
    pub fn into_parsed(self, language: Language) -> Query {
        match self {
            Self::Parsed(query) => query,
            Self::Unparsed(query_text) => Query::new(language, query_text).unwrap(),
        }
    }
}

impl<'query_text> From<Query> for ParsedOrUnparsedQuery<'query_text> {
    fn from(value: Query) -> Self {
        Self::Parsed(value)
    }
}

impl<'query_text> From<&'query_text str> for ParsedOrUnparsedQuery<'query_text> {
    fn from(value: &'query_text str) -> Self {
        Self::Unparsed(value)
    }
}

fn print_violation(violation: &Violation, query_match_context: &QueryMatchContext) {
    println!(
        "{:?}:{}:{} {} {}",
        query_match_context.path,
        violation.node.range().start_point.row + 1,
        violation.node.range().start_point.column + 1,
        violation.message,
        query_match_context.rule.name,
    );
}
