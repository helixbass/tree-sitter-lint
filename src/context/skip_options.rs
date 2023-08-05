use derive_builder::Builder;
use tree_sitter_grep::tree_sitter::Node;

#[derive(Builder)]
#[builder(default, setter(strip_option))]
pub struct SkipOptions<TFilter: FnMut(Node) -> bool> {
    skip: Option<usize>,
    include_comments: Option<bool>,
    filter: Option<TFilter>,
}

impl<TFilter: FnMut(Node) -> bool> SkipOptions<TFilter> {
    pub fn skip(&self) -> usize {
        self.skip.unwrap_or_default()
    }

    pub fn include_comments(&self) -> bool {
        self.include_comments.unwrap_or_default()
    }

    pub fn filter(&mut self) -> Option<&mut TFilter> {
        self.filter.as_mut()
    }
}

impl<TFilter: FnMut(Node) -> bool> Default for SkipOptions<TFilter> {
    fn default() -> Self {
        Self {
            skip: Default::default(),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl From<usize> for SkipOptions<fn(Node) -> bool> {
    fn from(value: usize) -> Self {
        Self {
            skip: Some(value),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl<TFilter: FnMut(Node) -> bool> From<TFilter> for SkipOptions<TFilter> {
    fn from(value: TFilter) -> Self {
        Self {
            skip: Default::default(),
            include_comments: Default::default(),
            filter: Some(value),
        }
    }
}
