use std::ops;

use squalid::{IsEmpty, OptionExt};
use tree_sitter_grep::tree_sitter::Node;

#[derive(Default)]
pub struct Fixer {
    pending_fixes: Option<Vec<PendingFix>>,
}

impl Fixer {
    pub fn replace_text(&mut self, node: Node, replacement: impl Into<String>) {
        self.pending_fixes
            .get_or_insert_with(Default::default)
            .push(PendingFix::new(node.byte_range(), replacement.into()));
    }

    pub(crate) fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending_fixes
            .as_ref()
            .is_none_or_matches(|pending_fixes| pending_fixes.is_empty())
    }

    pub fn remove_range(&mut self, range: ops::Range<usize>) {
        self.pending_fixes
            .get_or_insert_with(Default::default)
            .push(PendingFix::new(range, Default::default()));
    }

    pub fn replace_text_range(&mut self, range: ops::Range<usize>, replacement: impl Into<String>) {
        self.pending_fixes
            .get_or_insert_with(Default::default)
            .push(PendingFix::new(range, replacement.into()));
    }
}

impl IsEmpty for Fixer {
    fn _is_empty(&self) -> bool {
        self.is_empty()
    }
}

#[derive(Clone)]
pub struct PendingFix {
    pub range: ops::Range<usize>,
    pub replacement: String,
}

impl PendingFix {
    pub fn new(range: ops::Range<usize>, replacement: String) -> Self {
        Self { range, replacement }
    }
}
