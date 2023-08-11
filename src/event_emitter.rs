use better_any::Tid;
use once_cell::sync::Lazy;
use regex::Regex;
use tree_sitter_grep::{tree_sitter::Node, RopeOrSlice, SupportedLanguage};

pub type EventEmitterName = String;
pub type EventType = String;
pub type EventTypeIndex = usize;
pub type EventEmitterIndex = usize;

pub trait EventEmitterFactory: Send + Sync {
    fn name(&self) -> EventEmitterName;
    fn languages(&self) -> Vec<SupportedLanguage>;
    fn event_types(&self) -> Vec<EventType>;
    fn create<'a>(&self, file_contents: RopeOrSlice<'a>) -> Box<dyn EventEmitter<'a> + 'a>;
}

pub trait EventEmitter<'a>: Tid<'a> {
    fn enter_node(&mut self, node: Node<'a>) -> Option<Vec<EventTypeIndex>>;
    fn leave_node(&mut self, node: Node<'a>) -> Option<Vec<EventTypeIndex>>;
    fn processing_emitted_event_index(&mut self, index: usize);
}

const EVENT_EMITTER_LISTENER_PREFIX: &str = "__tree_sitter_lint_event_emitter_";

static EVENT_EMITTER_LISTENER: Lazy<Regex> = Lazy::new(|| {
    // TODO: validate that event emitter names/event names match this regex (if we're going to
    // restrict to it here)?
    Regex::new(&format!(r#"{EVENT_EMITTER_LISTENER_PREFIX}([a-zA-Z][a-zA-Z_-]*[a-zA-Z])__([a-zA-Z][a-zA-Z_-]*[a-zA-Z])"#)).unwrap()
});

pub fn is_listener(selector: &str) -> Option<(EventEmitterName, EventType)> {
    EVENT_EMITTER_LISTENER
        .captures(selector)
        .map(|captures| (captures[1].to_owned(), captures[2].to_owned()))
}

#[macro_export]
macro_rules! get_const_listener_selector {
    ($event_emitter_name:expr, $event_name:expr) => {
        $crate::const_format::formatcp!(
            "__tree_sitter_lint_event_emitter_{}__{}",
            $event_emitter_name,
            $event_name
        )
    };
}

pub fn get_listener_selector(
    event_emitter_name: &str, /* EventEmitterName */
    event_name: &str,         /* EventType */
) -> String {
    format!("{EVENT_EMITTER_LISTENER_PREFIX}{event_emitter_name}__{event_name}")
}
