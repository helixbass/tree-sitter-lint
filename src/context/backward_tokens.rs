use tree_sitter_grep::tree_sitter::Node;

use super::get_tokens::TokenWalkerState;

macro_rules! loop_landed_on_comment_or_node {
    ($self:expr) => {
        loop_if_on_comment!($self);
        loop_landed_on_node!($self);
    };
}

macro_rules! loop_if_on_comment {
    ($self:expr) => {
        if $self.node.kind() == "comment" {
            $self.state = OnComment;
            continue;
        }
    };
}

macro_rules! loop_landed_on_node {
    ($self:expr) => {
        $self.state = LandedOnNonCommentNode;
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
                    if self.node.kind() == "comment" {
                        loop_done!(self);
                    }
                    let num_children = self.node.child_count();
                    if num_children == 0 {
                        self.state = Done;
                        return Some(self.node);
                    }
                    self.node = self.node.child(num_children - 1).unwrap();
                    loop_landed_on_comment_or_node!(self);
                }
                ReturnedCurrentNode => {
                    move_to_prev_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_comment_or_node!(self);
                }
                OnComment => {
                    move_to_prev_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_comment_or_node!(self);
                }
                LandedOnNonCommentNode => {
                    let num_children = self.node.child_count();
                    if num_children == 0 {
                        self.state = ReturnedCurrentNode;
                        return Some(self.node);
                    }
                    self.node = self.node.child(num_children - 1).unwrap();
                    loop_landed_on_comment_or_node!(self);
                }
                JustReturnedToParent => {
                    if self.node == self.original_node {
                        loop_done!(self);
                    }
                    move_to_prev_sibling_or_go_to_parent_and_loop!(self);
                    loop_landed_on_comment_or_node!(self);
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
        test_backward_tokens_text("const x = 5;", &[";", "5", "=", "x", "const"]);
    }
}
