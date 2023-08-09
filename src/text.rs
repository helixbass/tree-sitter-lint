use std::{borrow::Cow, ops};

use crate::{tree_sitter::Node, tree_sitter_grep::RopeOrSlice};

pub trait SourceTextProvider<'a> {
    fn node_text(&self, node: Node) -> Cow<'a, str>;
}

impl<'a> SourceTextProvider<'a> for &'a [u8] {
    fn node_text(&self, node: Node) -> Cow<'a, str> {
        node.utf8_text(self).unwrap().into()
    }
}

impl<'a> SourceTextProvider<'a> for RopeOrSlice<'a> {
    fn node_text(&self, node: Node) -> Cow<'a, str> {
        get_text_slice(*self, node.byte_range())
    }
}

pub fn get_text_slice(file_contents: RopeOrSlice, range: ops::Range<usize>) -> Cow<'_, str> {
    match file_contents {
        RopeOrSlice::Slice(slice) => std::str::from_utf8(&slice[range]).unwrap().into(),
        RopeOrSlice::Rope(rope) => rope.byte_slice(range).into(),
    }
}
