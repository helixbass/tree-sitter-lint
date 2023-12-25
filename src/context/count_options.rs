use derive_builder::Builder;
use tree_sitter_grep::tree_sitter::Node;

#[derive(Builder)]
#[builder(default, setter(strip_option))]
pub struct CountOptions<TFilter: FnMut(Node) -> bool> {
    count: Option<usize>,
    include_comments: Option<bool>,
    filter: Option<TFilter>,
}

impl<TFilter: FnMut(Node) -> bool> CountOptions<TFilter> {
    pub fn count(&self) -> usize {
        self.count.unwrap_or_default()
    }

    pub fn include_comments(&self) -> bool {
        self.include_comments.unwrap_or_default()
    }

    pub fn filter(&mut self) -> Option<&mut TFilter> {
        self.filter.as_mut()
    }
}

impl<TFilter: FnMut(Node) -> bool> Default for CountOptions<TFilter> {
    fn default() -> Self {
        Self {
            count: Default::default(),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl From<usize> for CountOptions<fn(Node) -> bool> {
    fn from(value: usize) -> Self {
        Self {
            count: Some(value),
            include_comments: Default::default(),
            filter: Default::default(),
        }
    }
}

impl<TFilter: FnMut(Node) -> bool> From<TFilter> for CountOptions<TFilter> {
    fn from(value: TFilter) -> Self {
        Self {
            count: Default::default(),
            include_comments: Default::default(),
            filter: Some(value),
        }
    }
}
