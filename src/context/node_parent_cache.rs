use std::{collections::HashMap, sync::Arc};

use tree_sitter_grep::tree_sitter::{Node, Tree};

use crate::{walk_tree, TreeEnterLeaveVisitor};

pub type NodeParentCache<'a> = HashMap<Node<'a>, Node<'a>>;

#[derive(Default)]
struct NodeParentCachePopulator<'a> {
    cache: NodeParentCache<'a>,
    current_node_stack: Vec<Node<'a>>,
}

impl<'a> TreeEnterLeaveVisitor<'a> for NodeParentCachePopulator<'a> {
    fn enter_node(&mut self, node: Node<'a>) {
        if let Some(current_parent) = self.current_node_stack.last().copied() {
            self.cache.insert(node, current_parent);
        }

        self.current_node_stack.push(node);
    }

    fn leave_node(&mut self, node: Node<'a>) {
        assert!(node == self.current_node_stack.pop().unwrap());
    }
}

pub fn get_node_parent_cache(tree: &Tree) -> Arc<NodeParentCache> {
    let mut node_parent_cache_populator = NodeParentCachePopulator::default();
    walk_tree(tree, &mut node_parent_cache_populator);
    Arc::new(node_parent_cache_populator.cache)
}

pub trait NodeParentProvider<'a> {
    fn node_parent(&self, node: Node<'a>) -> Node<'a> {
        self.maybe_node_parent(node).unwrap()
    }
    fn maybe_node_parent(&self, node: Node<'a>) -> Option<Node<'a>>;
    fn standalone_node_parent_provider(&self) -> StandaloneNodeParentProvider<'a>;
}

#[derive(Clone)]
pub struct StandaloneNodeParentProvider<'a> {
    cache: Arc<NodeParentCache<'a>>,
}

impl<'a> From<Arc<NodeParentCache<'a>>> for StandaloneNodeParentProvider<'a> {
    fn from(cache: Arc<NodeParentCache<'a>>) -> Self {
        Self { cache }
    }
}

impl<'a> From<&'a Tree> for StandaloneNodeParentProvider<'a> {
    fn from(tree: &'a Tree) -> Self {
        Self {
            cache: get_node_parent_cache(tree),
        }
    }
}

impl<'a> NodeParentProvider<'a> for StandaloneNodeParentProvider<'a> {
    fn maybe_node_parent(&self, node: Node<'a>) -> Option<Node<'a>> {
        self.cache.get(&node).copied()
    }

    fn standalone_node_parent_provider(&self) -> StandaloneNodeParentProvider<'a> {
        self.clone()
    }
}
