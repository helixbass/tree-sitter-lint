use std::path::Path;

use tree_sitter::Language;

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
}

impl<'a> QueryMatchContext<'a> {
    pub fn new(path: &'a Path, file_contents: &'a [u8], rule: &'a ResolvedRule) -> Self {
        Self {
            path,
            file_contents,
            rule,
        }
    }

    pub fn report(&self, violation: Violation) {
        print_violation(&violation, self);
    }
}

fn print_violation(violation: &Violation, query_match_context: &QueryMatchContext) {
    eprintln!(
        "{:?}:{}:{} {} {}",
        query_match_context.path,
        violation.node.range().start_point.row + 1,
        violation.node.range().start_point.column + 1,
        violation.message,
        query_match_context.rule.name,
    );
}
