use std::{ops, path::Path};

use tree_sitter::{Language, Node, Query, QueryCursor};

use crate::{
    rule::ResolvedRule,
    violation::{Violation, ViolationWithContext},
    Config,
};

pub struct QueryMatchContext<'a> {
    pub path: &'a Path,
    pub file_contents: &'a [u8],
    pub rule: &'a ResolvedRule<'a>,
    config: &'a Config,
    pending_fixes: Option<Vec<PendingFix>>,
    pub violations: Option<Vec<ViolationWithContext>>,
}

impl<'a> QueryMatchContext<'a> {
    pub fn new(
        path: &'a Path,
        file_contents: &'a [u8],
        rule: &'a ResolvedRule,
        config: &'a Config,
    ) -> Self {
        Self {
            path,
            file_contents,
            rule,
            config,
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
            }
        }
        let violation = violation.contextualize(self);
        self.violations
            .get_or_insert_with(Default::default)
            .push(violation);
    }

    pub fn get_node_text(&self, node: Node) -> &str {
        node.utf8_text(self.file_contents).unwrap()
    }

    pub fn maybe_get_single_matching_node_for_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        let query = query.into().into_parsed(self.config.language.language());
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

    pub fn get_number_of_query_matches<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> usize {
        let query = query.into().into_parsed(self.config.language.language());
        let mut query_cursor = QueryCursor::new();
        query_cursor
            .matches(&query, enclosing_node, self.file_contents)
            .count()
    }

    pub fn pending_fixes(&self) -> Option<&[PendingFix]> {
        self.pending_fixes.as_deref()
    }

    pub fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes
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
