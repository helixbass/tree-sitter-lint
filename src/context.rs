use std::path::Path;

use tree_sitter::Language;

use crate::violation::Violation;

pub struct Context {
    pub language: Language,
}

impl Context {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    pub fn report(&self, violation: Violation) {
        print_violation(&violation);
    }
}

fn print_violation(violation: &Violation) {
    eprintln!(
        "{:?}:{}:{} {}",
        violation.query_match_context.path,
        violation.node.range().start_point.row + 1,
        violation.node.range().start_point.column + 1,
        violation.message
    );
}

pub struct QueryMatchContext<'path> {
    pub path: &'path Path,
}

impl<'path> QueryMatchContext<'path> {
    pub fn new(path: &'path Path) -> Self {
        Self { path }
    }
}
