use std::rc::Rc;

use derive_builder::Builder;
use tree_sitter::Node;

use crate::context::Fixer;

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'a> {
    pub message: String,
    pub node: Node<'a>,
    #[builder(default, setter(custom))]
    pub fix: Option<Rc<dyn Fn(&mut Fixer) + 'a>>,
}

impl<'a> ViolationBuilder<'a> {
    pub fn fix(&mut self, callback: impl Fn(&mut Fixer) + 'a) -> &mut Self {
        self.fix = Some(Some(Rc::new(callback)));
        self
    }
}
