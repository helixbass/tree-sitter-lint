use std::{path::PathBuf, rc::Rc};

use derive_builder::Builder;
use tree_sitter::Node;

use crate::{
    config::PluginIndex,
    context::{Fixer, QueryMatchContext},
    rule::RuleMeta,
    Config,
};

#[derive(Builder)]
#[builder(setter(into))]
pub struct Violation<'a> {
    pub message: String,
    pub node: Node<'a>,
    #[allow(clippy::type_complexity)]
    #[builder(default, setter(custom))]
    pub fix: Option<Rc<dyn Fn(&mut Fixer) + 'a>>,
}

impl<'a> Violation<'a> {
    pub fn contextualize(
        self,
        query_match_context: &QueryMatchContext<'a>,
    ) -> ViolationWithContext {
        let Violation { message, node, fix } = self;
        ViolationWithContext {
            message,
            range: node.range(),
            path: query_match_context.path.to_owned(),
            rule: query_match_context.rule.meta.clone(),
            plugin_index: query_match_context.rule.plugin_index,
            was_fix: fix.is_some(),
        }
    }
}

impl<'a> ViolationBuilder<'a> {
    pub fn fix(&mut self, callback: impl Fn(&mut Fixer) + 'a) -> &mut Self {
        self.fix = Some(Some(Rc::new(callback)));
        self
    }
}

#[derive(Clone)]
pub struct ViolationWithContext {
    pub message: String,
    pub range: tree_sitter::Range,
    pub path: PathBuf,
    pub rule: RuleMeta,
    pub plugin_index: Option<PluginIndex>,
    pub was_fix: bool,
}

impl ViolationWithContext {
    pub fn print(&self, config: &Config) {
        println!(
            "{:?}:{}:{} {} {}",
            self.path,
            self.range.start_point.row + 1,
            self.range.start_point.column + 1,
            self.message,
            match self.plugin_index {
                None => self.rule.name.clone(),
                Some(plugin_index) => format!(
                    "{}/{}",
                    config.get_plugin_name(plugin_index),
                    self.rule.name
                ),
            }
        );
    }
}

#[macro_export]
macro_rules! violation {
    ($($variant:ident => $value:expr),* $(,)?) => {
        $crate::builder_args!(
            $crate::ViolationBuilder,
            $($variant => $value),*,
        )
    }
}
