use std::sync::Arc;

use proc_macros::{
    rule_crate_internal as rule, rule_tests_crate_internal as rule_tests,
    violation_crate_internal as violation,
};
use tree_sitter_grep::{tree_sitter::Node, RopeOrSlice, SupportedLanguage};

use crate::{
    event_emitter::{get_listener_selector, EventEmitterName, EventType},
    EventEmitter, EventEmitterFactory, EventTypeIndex, Plugin, RuleTester,
};

#[test]
fn test_event_emitter() {
    RuleTester::run_with_plugins(
        rule! {
            name => "listens-for-event",
            listeners => [
                get_listener_selector("dummy", "dummy-event") => |node, context| {
                    context.report(violation! {
                        node => node,
                        message => "whee",
                    });
                }
            ],
            languages => [Rust],
        },
        rule_tests! {
            valid => [
                r#"
                    use foo::bar;
                "#,
            ],
            invalid => [
                {
                    code => r#"
                        fn foo() {}
                    "#,
                    errors => [r#"whee"#],
                },
            ]
        },
        vec![get_plugin_with_event_emitter()],
    );
}

fn get_plugin_with_event_emitter() -> Plugin {
    Plugin {
        name: "has-event-emitter".to_owned(),
        rules: Default::default(),
        event_emitter_factories: vec![Arc::new(DummyEventEmitterFactory)],
    }
}

struct DummyEventEmitterFactory;

impl EventEmitterFactory for DummyEventEmitterFactory {
    fn name(&self) -> EventEmitterName {
        "dummy".to_owned()
    }

    fn languages(&self) -> Vec<SupportedLanguage> {
        vec![SupportedLanguage::Rust]
    }

    fn event_types(&self) -> Vec<EventType> {
        vec!["dummy-event".to_owned()]
    }

    fn create<'a>(&self, _file_contents: RopeOrSlice<'a>) -> Box<dyn EventEmitter<'a>> {
        Box::new(DummyEventEmitter)
    }
}

struct DummyEventEmitter;

impl<'a> EventEmitter<'a> for DummyEventEmitter {
    fn enter_node(&mut self, node: Node<'a>) -> Option<Vec<EventTypeIndex>> {
        if node.kind() == "function_item" {
            Some(vec![0])
        } else {
            None
        }
    }

    fn leave_node(&mut self, _node: Node<'a>) -> Option<Vec<EventTypeIndex>> {
        None
    }
}
