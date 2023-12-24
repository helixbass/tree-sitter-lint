use tree_sitter_grep::tree_sitter::{Node, TreeCursor};

use crate::NodeExt;

macro_rules! move_to_next_sibling_or_go_to_parent_and_loop {
    ($self:expr) => {
        if !$self.cursor.goto_next_sibling() {
            $self.cursor.goto_parent();
            $self.state = JustReturnedToParent;
            continue;
        }
    };
}

macro_rules! move_to_next_sibling_or_try_go_to_parent_and_loop {
    ($self:expr) => {
        if !$self.cursor.goto_next_sibling() {
            if !$self.cursor.goto_parent() {
                $self.state = Done;
                continue;
            }
            $self.state = JustReturnedToParent;
            continue;
        }
    };
}

macro_rules! loop_landed_on_node {
    ($self:expr) => {
        $self.state = LandedOnNode;
        continue;
    };
}

macro_rules! loop_done {
    ($self:expr) => {
        $self.state = Done;
        continue;
    };
}

pub fn get_tokens(node: Node) -> TokenWalker {
    TokenWalker::new(node)
}

pub struct TokenWalker<'a> {
    state: TokenWalkerState,
    cursor: TreeCursor<'a>,
    original_node: Node<'a>,
}

impl<'a> TokenWalker<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::Initial,
            cursor: node.walk(),
            original_node: node,
        }
    }
}

impl<'a> Iterator for TokenWalker<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use TokenWalkerState::*;

        loop {
            match self.state {
                Done => {
                    return None;
                }
                Initial => {
                    if !self.cursor.goto_first_child() {
                        self.state = Done;
                        return Some(self.cursor.node());
                    }
                    loop_landed_on_node!(self);
                }
                ReturnedCurrentNode => {
                    move_to_next_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
                LandedOnNode => {
                    if !self.cursor.goto_first_child() {
                        self.state = ReturnedCurrentNode;
                        return Some(self.cursor.node());
                    }
                    loop_landed_on_node!(self);
                }
                JustReturnedToParent => {
                    if self.cursor.node() == self.original_node {
                        loop_done!(self);
                    }
                    move_to_next_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TokenWalkerState {
    Initial,
    ReturnedCurrentNode,
    JustReturnedToParent,
    LandedOnNode,
    Done,
}

#[allow(dead_code)]
pub fn get_tokens_including_after_node(node: Node) -> impl Iterator<Item = Node> {
    TokenWalkerUntilEndOfFile::new(node)
}

pub fn get_tokens_after_node(node: Node) -> impl Iterator<Item = Node> {
    TokenWalkerUntilEndOfFile::for_after_node(node)
}

struct TokenWalkerUntilEndOfFile<'a> {
    state: TokenWalkerState,
    cursor: TreeCursor<'a>,
}

impl<'a> TokenWalkerUntilEndOfFile<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::Initial,
            cursor: node.get_cursor_scoped_to_root(),
        }
    }

    pub fn for_after_node(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::JustReturnedToParent,
            cursor: node.get_cursor_scoped_to_root(),
        }
    }
}

impl<'a> Iterator for TokenWalkerUntilEndOfFile<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use TokenWalkerState::*;

        loop {
            match self.state {
                Done => {
                    return None;
                }
                Initial => {
                    loop_landed_on_node!(self);
                }
                ReturnedCurrentNode => {
                    move_to_next_sibling_or_try_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
                LandedOnNode => {
                    if !self.cursor.goto_first_child() {
                        self.state = ReturnedCurrentNode;
                        return Some(self.cursor.node());
                    }
                    loop_landed_on_node!(self);
                }
                JustReturnedToParent => {
                    move_to_next_sibling_or_try_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter_grep::{tree_sitter::Parser, SupportedLanguage};

    use super::*;

    fn test_all_tokens_text(text: &str, all_tokens_text: &[&str]) {
        let mut parser = Parser::new();
        parser
            .set_language(SupportedLanguage::Javascript.language(None))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            get_tokens(tree.root_node())
                .map(|node| node.utf8_text(text.as_bytes()).unwrap())
                .collect::<Vec<_>>(),
            all_tokens_text
        );
    }

    #[test]
    fn test_get_tokens_simple() {
        test_all_tokens_text("const x = 5;", &["const", "x", "=", "5", ";"]);
    }

    #[test]
    fn test_get_tokens_structured() {
        test_all_tokens_text(
            r#"
            const whee = function(foo) {
                for (let x = 1; x < 100; x++) {
                    foo(x);
                }
            }
        "#,
            &[
                "const", "whee", "=", "function", "(", "foo", ")", "{", "for", "(", "let", "x",
                "=", "1", ";", "x", "<", "100", ";", "x", "++", ")", "{", "foo", "(", "x", ")",
                ";", "}", "}",
            ],
        );
    }

    fn test_all_tokens_including_after_node_text(
        text: &str,
        get_node: impl FnOnce(Node) -> Node,
        all_tokens_text: &[&str],
    ) {
        let mut parser = Parser::new();
        parser
            .set_language(SupportedLanguage::Javascript.language(None))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            get_tokens_including_after_node(get_node(tree.root_node()))
                .map(|node| node.utf8_text(text.as_bytes()).unwrap())
                .collect::<Vec<_>>(),
            all_tokens_text
        );
    }

    #[test]
    fn test_get_tokens_including_after_node() {
        test_all_tokens_including_after_node_text(
            r#"
                import {foo} from "bar";

                const x = 5;
                let y = z;
            "#,
            |root_node| root_node.named_child(1).unwrap().named_child(0).unwrap(),
            &["x", "=", "5", ";", "let", "y", "=", "z", ";"],
        );
    }

    fn test_all_tokens_after_node_text(
        text: &str,
        get_node: impl FnOnce(Node) -> Node,
        all_tokens_text: &[&str],
    ) {
        let mut parser = Parser::new();
        parser
            .set_language(SupportedLanguage::Javascript.language(None))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            get_tokens_after_node(get_node(tree.root_node()))
                .map(|node| node.utf8_text(text.as_bytes()).unwrap())
                .collect::<Vec<_>>(),
            all_tokens_text
        );
    }

    #[test]
    fn test_get_tokens_after_node() {
        test_all_tokens_after_node_text(
            r#"
                import {foo} from "bar";

                const x = 5;
                let y = z;
            "#,
            |root_node| root_node.find_first_descendant_of_kind("number").unwrap(),
            &[";", "let", "y", "=", "z", ";"],
        );
    }
}
