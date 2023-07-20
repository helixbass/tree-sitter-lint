use tree_sitter::Language;

use crate::violation::Violation;

pub struct Context {
    language: Language,
}

impl Context {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    pub fn report(&self, violation: Violation) {
        unimplemented!()
    }
}
