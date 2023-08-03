use tree_sitter_grep::tree_sitter::Node;

use super::get_tokens::TokenWalkerState;

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

macro_rules! move_to_prev_sibling_or_go_to_parent_and_loop {
    ($self:expr) => {
        match $self.node.prev_sibling() {
            None => {
                $self.node = $self.node.parent().unwrap();
                $self.state = JustReturnedToParent;
                continue;
            }
            Some(prev_sibling) => {
                $self.node = prev_sibling;
            }
        }
    };
}

macro_rules! move_to_prev_sibling_or_try_go_to_parent_and_loop {
    ($self:expr) => {
        match $self.node.prev_sibling() {
            None => {
                $self.node = match $self.node.parent() {
                    None => {
                        $self.state = Done;
                        continue;
                    }
                    Some(parent) => parent,
                };
                $self.state = JustReturnedToParent;
                continue;
            }
            Some(prev_sibling) => {
                $self.node = prev_sibling;
            }
        }
    };
}

pub fn get_backward_tokens(node: Node) -> impl Iterator<Item = Node> {
    BackwardTokenWalker::new(node)
}

struct BackwardTokenWalker<'a> {
    state: TokenWalkerState,
    node: Node<'a>,
    original_node: Node<'a>,
}

impl<'a> BackwardTokenWalker<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::Initial,
            node,
            original_node: node,
        }
    }
}

impl<'a> Iterator for BackwardTokenWalker<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use TokenWalkerState::*;

        loop {
            match self.state {
                Done => {
                    return None;
                }
                Initial => {
                    let num_children = self.node.child_count();
                    if num_children == 0 {
                        self.state = Done;
                        return Some(self.node);
                    }
                    self.node = self.node.child(num_children - 1).unwrap();
                    loop_landed_on_node!(self);
                }
                ReturnedCurrentNode => {
                    move_to_prev_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
                LandedOnNode => {
                    let num_children = self.node.child_count();
                    if num_children == 0 {
                        self.state = ReturnedCurrentNode;
                        return Some(self.node);
                    }
                    self.node = self.node.child(num_children - 1).unwrap();
                    loop_landed_on_node!(self);
                }
                JustReturnedToParent => {
                    if self.node == self.original_node {
                        loop_done!(self);
                    }
                    move_to_prev_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn get_tokens_including_before_node(node: Node) -> impl Iterator<Item = Node> {
    TokenWalkerUntilBeginningOfFile::new(node)
}

pub fn get_tokens_before_node(node: Node) -> impl Iterator<Item = Node> {
    TokenWalkerUntilBeginningOfFile::for_before_node(node)
}

struct TokenWalkerUntilBeginningOfFile<'a> {
    state: TokenWalkerState,
    node: Node<'a>,
}

impl<'a> TokenWalkerUntilBeginningOfFile<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::Initial,
            node,
        }
    }

    pub fn for_before_node(node: Node<'a>) -> Self {
        Self {
            state: TokenWalkerState::JustReturnedToParent,
            node,
        }
    }
}

impl<'a> Iterator for TokenWalkerUntilBeginningOfFile<'a> {
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
                    move_to_prev_sibling_or_try_go_to_parent_and_loop!(self);
                    loop_landed_on_node!(self);
                }
                LandedOnNode => {
                    let num_children = self.node.child_count();
                    if num_children == 0 {
                        self.state = ReturnedCurrentNode;
                        return Some(self.node);
                    }
                    self.node = self.node.child(num_children - 1).unwrap();
                    loop_landed_on_node!(self);
                }
                JustReturnedToParent => {
                    move_to_prev_sibling_or_try_go_to_parent_and_loop!(self);
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

    fn test_backward_tokens_text(text: &str, all_tokens_text: &[&str]) {
        let mut parser = Parser::new();
        parser
            .set_language(SupportedLanguage::Javascript.language())
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert_eq!(
            get_backward_tokens(tree.root_node())
                .map(|node| node.utf8_text(text.as_bytes()).unwrap())
                .collect::<Vec<_>>(),
            all_tokens_text
        );
    }

    #[test]
    fn test_get_backward_tokens_simple() {
        test_backward_tokens_text(
            "const x = 5;",
            &["const", "x", "=", "5", ";"]
                .into_iter()
                .rev()
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_get_backward_tokens_structured() {
        test_backward_tokens_text(
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
            ]
            .into_iter()
            .rev()
            .collect::<Vec<_>>(),
        );
    }
}
