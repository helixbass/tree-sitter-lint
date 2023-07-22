use std::{path::PathBuf, rc::Rc};

use derive_builder::Builder;
use tree_sitter::Node;

use crate::{
    context::{Fixer, QueryMatchContext},
    rule::RuleMeta,
};

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'a> {
    pub message: String,
    pub node: Node<'a>,
    #[builder(default, setter(custom))]
    pub fix: Option<Rc<dyn Fn(&mut Fixer) + 'a>>,
}

impl<'a> Violation<'a> {
    pub fn contextualize(
        self,
        query_match_context: &QueryMatchContext<'a>,
    ) -> ViolationWithContext {
        let Violation { message, node, .. } = self;
        ViolationWithContext {
            message,
            range: node.range(),
            path: query_match_context.path.to_owned(),
            rule: query_match_context.rule.meta.clone(),
        }
    }
}

impl<'a> ViolationBuilder<'a> {
    pub fn fix(&mut self, callback: impl Fn(&mut Fixer) + 'a) -> &mut Self {
        self.fix = Some(Some(Rc::new(callback)));
        self
    }
}

pub struct ViolationWithContext {
    pub message: String,
    pub range: tree_sitter::Range,
    pub path: PathBuf,
    pub rule: RuleMeta,
}

impl ViolationWithContext {
    pub fn print(&self) {
        println!(
            "{:?}:{}:{} {} {}",
            self.path,
            self.range.start_point.row + 1,
            self.range.start_point.column + 1,
            self.message,
            self.rule.name,
        );
    }
}
