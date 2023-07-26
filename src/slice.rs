use std::ops;

use tree_sitter_grep::{ropey::Rope, RopeOrSlice};

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
