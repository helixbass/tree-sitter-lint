use std::{iter, ops};

use ouroboros::self_referencing;
use ropey::{iter::Chunks, Rope, RopeSlice};
use tree_sitter_grep::{
    tree_sitter::{Node, Parser, TextProvider, Tree},
    Parseable,
};

#[derive(Copy, Clone)]
pub enum RopeOrSlice<'a> {
    Slice(&'a [u8]),
    Rope(&'a Rope),
}

impl<'a> TextProvider<'a> for RopeOrSlice<'a> {
    type I = RopeOrSliceTextProviderIterator<'a>;

    fn text(&mut self, node: Node) -> Self::I {
        match self {
            Self::Slice(slice) => {
                RopeOrSliceTextProviderIterator::Slice(iter::once(&slice[node.byte_range()]))
            }
            Self::Rope(rope) => {
                let rope_slice = rope.byte_slice(node.byte_range());
                RopeOrSliceTextProviderIterator::Rope(RopeOrSliceRopeTextProviderIterator::new(
                    rope_slice,
                    |rope_slice| rope_slice.chunks(),
                ))
            }
        }
    }
}

impl<'a> Parseable for RopeOrSlice<'a> {
    fn parse(&self, parser: &mut Parser, old_tree: Option<&Tree>) -> Option<Tree> {
        match self {
            Self::Slice(slice) => slice.parse(parser, old_tree),
            Self::Rope(rope) => parser.parse_with(
                &mut |byte_offset, _| {
                    let (chunk, chunk_start_byte_index, _, _) = rope.chunk_at_byte(byte_offset);
                    &chunk[byte_offset - chunk_start_byte_index..]
                },
                old_tree,
            ),
        }
    }
}

impl<'a> From<&'a [u8]> for RopeOrSlice<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self::Slice(value)
    }
}

impl<'a> From<&'a Rope> for RopeOrSlice<'a> {
    fn from(value: &'a Rope) -> Self {
        Self::Rope(value)
    }
}

pub enum RopeOrSliceTextProviderIterator<'a> {
    Slice(iter::Once<&'a [u8]>),
    Rope(RopeOrSliceRopeTextProviderIterator<'a>),
}

impl<'a> Iterator for RopeOrSliceTextProviderIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Slice(slice_iterator) => slice_iterator.next(),
            Self::Rope(rope_iterator) => rope_iterator.next().map(str::as_bytes),
        }
    }
}

#[self_referencing]
pub struct RopeOrSliceRopeTextProviderIterator<'a> {
    rope_slice: RopeSlice<'a>,

    #[borrows(rope_slice)]
    chunks_iterator: Chunks<'a>,
}

impl<'a> Iterator for RopeOrSliceRopeTextProviderIterator<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.with_chunks_iterator_mut(|chunks_iterator| chunks_iterator.next())
    }
}

pub enum MutRopeOrSlice<'a> {
    Rope(&'a mut Rope),
    Slice(&'a mut Vec<u8>),
}

impl<'a> MutRopeOrSlice<'a> {
    pub fn splice(&mut self, range: ops::Range<usize>, replacement: &str) {
        match self {
            MutRopeOrSlice::Rope(rope) => {
                rope.remove(range.clone());
                rope.insert(range.start, replacement);
            }
            MutRopeOrSlice::Slice(slice) => {
                slice.splice(range, replacement.bytes());
            }
        }
    }
}

impl<'a> TextProvider<'a> for &'a MutRopeOrSlice<'a> {
    type I = RopeOrSliceTextProviderIterator<'a>;

    fn text(&mut self, node: Node) -> Self::I {
        <RopeOrSlice<'a>>::from(&**self).text(node)
    }
}

impl<'a> Parseable for &'a MutRopeOrSlice<'a> {
    fn parse(&self, parser: &mut Parser, old_tree: Option<&Tree>) -> Option<Tree> {
        <RopeOrSlice<'a>>::from(*self).parse(parser, old_tree)
    }
}

impl<'a> From<&'a mut Rope> for MutRopeOrSlice<'a> {
    fn from(value: &'a mut Rope) -> Self {
        Self::Rope(value)
    }
}

impl<'a> From<&'a mut Vec<u8>> for MutRopeOrSlice<'a> {
    fn from(value: &'a mut Vec<u8>) -> Self {
        Self::Slice(value)
    }
}

impl<'a> From<&'a MutRopeOrSlice<'a>> for RopeOrSlice<'a> {
    fn from(value: &'a MutRopeOrSlice<'a>) -> Self {
        match value {
            MutRopeOrSlice::Rope(rope) => (&**rope).into(),
            MutRopeOrSlice::Slice(slice) => (&***slice).into(),
        }
    }
}
