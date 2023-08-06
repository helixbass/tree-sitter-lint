use std::{borrow::Cow, collections::HashMap, path::PathBuf, rc::Rc};

use derive_builder::Builder;
use tree_sitter_grep::tree_sitter::Range;

use crate::{
    config::PluginIndex,
    context::{Fixer, QueryMatchContext},
    rule::RuleMeta,
    tree_sitter::{self, Node},
    Config, FromFileRunContextInstanceProvider,
};

#[derive(Builder)]
#[builder(setter(into, strip_option))]
pub struct Violation<'a> {
    pub message_or_message_id: MessageOrMessageId,
    pub node: Node<'a>,
    #[allow(clippy::type_complexity)]
    #[builder(default, setter(custom))]
    pub fix: Option<Rc<dyn Fn(&mut Fixer) + 'a>>,
    #[builder(default)]
    pub data: Option<ViolationData>,
    #[builder(default)]
    pub range: Option<Range>,
}

impl<'a> Violation<'a> {
    pub fn contextualize<
        TFromFileRunContextInstanceProvider: FromFileRunContextInstanceProvider,
    >(
        self,
        query_match_context: &QueryMatchContext<TFromFileRunContextInstanceProvider>,
        had_fixes: bool,
    ) -> ViolationWithContext {
        let Violation {
            message_or_message_id,
            node,
            data,
            range,
            ..
        } = self;
        ViolationWithContext {
            message_or_message_id,
            range: range.unwrap_or_else(|| node.range()),
            kind: node.kind(),
            path: query_match_context.file_run_context.path.to_owned(),
            rule: query_match_context.rule.meta.clone(),
            plugin_index: query_match_context.rule.plugin_index,
            had_fixes,
            data,
        }
    }
}

impl<'a> ViolationBuilder<'a> {
    pub fn fix(&mut self, callback: impl Fn(&mut Fixer) + 'a) -> &mut Self {
        self.fix = Some(Some(Rc::new(callback)));
        self
    }

    pub fn message(&mut self, message: impl Into<String>) -> &mut Self {
        let message = message.into();
        self.message_or_message_id = Some(MessageOrMessageId::Message(message));
        self
    }

    pub fn message_id(&mut self, message_id: impl Into<String>) -> &mut Self {
        let message_id = message_id.into();
        self.message_or_message_id = Some(MessageOrMessageId::MessageId(message_id));
        self
    }
}

#[derive(Clone, Debug)]
pub enum MessageOrMessageId {
    Message(String),
    MessageId(String),
}

pub type ViolationData = HashMap<String, String>;

#[derive(Clone, Debug)]
pub struct ViolationWithContext {
    pub message_or_message_id: MessageOrMessageId,
    pub range: tree_sitter::Range,
    pub path: PathBuf,
    pub rule: RuleMeta,
    pub plugin_index: Option<PluginIndex>,
    pub had_fixes: bool,
    pub kind: &'static str,
    pub data: Option<ViolationData>,
}

impl ViolationWithContext {
    pub fn print(&self, config: &Config<impl FromFileRunContextInstanceProvider>) {
        println!(
            "{:?}:{}:{} {} {}",
            self.path,
            self.range.start_point.row + 1,
            self.range.start_point.column + 1,
            self.message(),
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

    pub fn message(&self) -> Cow<'_, str> {
        let message_template = match &self.message_or_message_id {
            MessageOrMessageId::Message(message) => message,
            MessageOrMessageId::MessageId(message_id) => self
                .rule
                .messages
                .as_ref()
                .expect("No messages for rule")
                .get(message_id)
                .unwrap_or_else(|| panic!("Invalid message ID for rule: {message_id:?}")),
        };
        format_message(message_template, self.data.as_ref())
    }
}

fn format_message<'a>(message_template: &'a str, data: Option<&ViolationData>) -> Cow<'a, str> {
    let mut formatted: Option<String> = Default::default();
    let mut unprocessed = message_template;
    while let Some(interpolation_offset) = unprocessed.find("{{") {
        let formatted = formatted.get_or_insert_with(Default::default);
        formatted.push_str(&unprocessed[..interpolation_offset]);
        unprocessed = &unprocessed[interpolation_offset + 2..];
        let end_interpolation_offset = unprocessed.find("}}").expect("No matching `}}` in message");
        let interpolated_name = unprocessed[..end_interpolation_offset].trim();
        let value = data
            .expect("No data provided for interpolated message")
            .get(interpolated_name)
            .unwrap_or_else(|| panic!("Didn't provide expected data key {interpolated_name:?}"));
        formatted.push_str(value);
        unprocessed = &unprocessed[end_interpolation_offset + 2..];
    }
    formatted
        .map(|mut formatted| {
            formatted.push_str(unprocessed);
            formatted
        })
        .map_or_else(|| message_template.into(), Into::into)
}
