use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    ops,
    path::Path,
    sync::Arc,
};

use better_any::TidAble;
use tree_sitter_grep::{
    streaming_iterator::StreamingIterator, tree_sitter::Tree, RopeOrSlice, SupportedLanguage,
};

mod backward_tokens;
mod fix;
mod get_tokens;
mod skip_options;

use backward_tokens::{get_backward_tokens, get_tokens_before_node};
pub use fix::{Fixer, PendingFix};
use get_tokens::{get_tokens, get_tokens_after_node};
pub use skip_options::{SkipOptions, SkipOptionsBuilder};

use crate::{
    rule::InstantiatedRule,
    tree_sitter::{Language, Node, Query},
    violation::{Violation, ViolationWithContext},
    AggregatedQueries, Config,
};

pub struct FileRunContext<
    'a,
    'b,
    TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
> {
    pub path: &'a Path,
    pub file_contents: RopeOrSlice<'a>,
    pub tree: &'a Tree,
    pub config: &'a Config<TFromFileRunContextInstanceProviderFactory>,
    pub language: SupportedLanguage,
    pub(crate) aggregated_queries:
        &'a AggregatedQueries<'a, TFromFileRunContextInstanceProviderFactory>,
    pub(crate) query: &'a Arc<Query>,
    pub(crate) instantiated_rules:
        &'a [InstantiatedRule<TFromFileRunContextInstanceProviderFactory>],
    from_file_run_context_instance_provider:
        &'b TFromFileRunContextInstanceProviderFactory::Provider<'a>,
}

impl<
        'a,
        'b,
        TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
    > Copy for FileRunContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>
{
}

impl<
        'a,
        'b,
        TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
    > Clone for FileRunContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>
{
    fn clone(&self) -> Self {
        Self {
            path: self.path,
            file_contents: self.file_contents,
            tree: self.tree,
            config: self.config,
            language: self.language,
            aggregated_queries: self.aggregated_queries,
            query: self.query,
            instantiated_rules: self.instantiated_rules,
            from_file_run_context_instance_provider: self.from_file_run_context_instance_provider,
        }
    }
}

impl<
        'a,
        'b,
        TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
    > FileRunContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: &'a Path,
        file_contents: impl Into<RopeOrSlice<'a>>,
        tree: &'a Tree,
        config: &'a Config<TFromFileRunContextInstanceProviderFactory>,
        language: SupportedLanguage,
        aggregated_queries: &'a AggregatedQueries<TFromFileRunContextInstanceProviderFactory>,
        query: &'a Arc<Query>,
        instantiated_rules: &'a [InstantiatedRule<TFromFileRunContextInstanceProviderFactory>],
        from_file_run_context_instance_provider: &'b TFromFileRunContextInstanceProviderFactory::Provider<'a>,
    ) -> Self {
        let file_contents = file_contents.into();
        Self {
            path,
            file_contents,
            tree,
            config,
            language,
            aggregated_queries,
            query,
            instantiated_rules,
            from_file_run_context_instance_provider,
        }
    }
}

// pub trait FromFileRunContextInstanceProvider<'b> {
//     fn get<'a: 'b>(&self, id: TypeId, file_run_context: FileRunContext<'a, '_>)
//         -> Option<&dyn Tid>;
// }

pub trait FromFileRunContextInstanceProvider<'a>: Sized {
    type Parent: FromFileRunContextInstanceProviderFactory<Provider<'a> = Self>;

    fn get<T: FromFileRunContext<'a> + for<'b> TidAble<'b>>(
        &self,
        file_run_context: FileRunContext<'a, '_, Self::Parent>,
    ) -> Option<&T>;
}

pub trait FromFileRunContextInstanceProviderFactory: Send + Sync {
    type Provider<'a>: FromFileRunContextInstanceProvider<'a, Parent = Self>;

    fn create<'a>(&self) -> Self::Provider<'a>;
}

pub trait FromFileRunContext<'a> {
    fn from_file_run_context(
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Self;
}

pub struct QueryMatchContext<
    'a,
    'b,
    TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
> {
    pub file_run_context: FileRunContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>,
    pub(crate) rule: &'a InstantiatedRule<TFromFileRunContextInstanceProviderFactory>,
    pending_fixes: RefCell<Option<Vec<PendingFix>>>,
    pub(crate) violations: RefCell<Option<Vec<ViolationWithContext>>>,
}

impl<
        'a,
        'b,
        TFromFileRunContextInstanceProviderFactory: FromFileRunContextInstanceProviderFactory,
    > QueryMatchContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>
{
    pub fn new(
        file_run_context: FileRunContext<'a, 'b, TFromFileRunContextInstanceProviderFactory>,
        rule: &'a InstantiatedRule<TFromFileRunContextInstanceProviderFactory>,
    ) -> Self {
        Self {
            file_run_context,
            rule,
            pending_fixes: Default::default(),
            violations: Default::default(),
        }
    }

    pub fn report(&self, violation: Violation) {
        let mut had_fixes = false;
        if self.file_run_context.config.fix {
            if let Some(fix) = violation.fix.as_ref() {
                if !self.rule.meta.fixable {
                    panic!("Rule {:?} isn't declared as fixable", self.rule.meta.name);
                }
                let mut fixer = Fixer::default();
                fix(&mut fixer);
                if let Some(pending_fixes) = fixer.into_pending_fixes() {
                    had_fixes = true;
                    self.pending_fixes
                        .borrow_mut()
                        .get_or_insert_with(Default::default)
                        .extend(pending_fixes);
                }
                if !self.file_run_context.config.report_fixed_violations {
                    return;
                }
            }
        }
        let violation = violation.contextualize(self, had_fixes);
        self.violations
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .push(violation);
    }

    pub fn get_node_text(&self, node: Node) -> Cow<'a, str> {
        get_node_text(node, self.file_run_context.file_contents)
    }

    pub fn maybe_get_single_captured_node_for_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        self.maybe_get_single_captured_node_for_filtered_query(query, |_| true, enclosing_node)
    }

    pub fn maybe_get_single_captured_node_for_filtered_query<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        mut predicate: impl FnMut(Node) -> bool,
        enclosing_node: Node<'enclosing_node>,
    ) -> Option<Node<'enclosing_node>> {
        let query = query
            .into()
            .into_parsed(self.file_run_context.language.language());
        let captures = tree_sitter_grep::get_captures_for_enclosing_node(
            self.file_run_context.file_contents,
            &query,
            0,
            None,
            enclosing_node,
        );
        let mut filtered_captures = captures
            .filter_map(|capture_info| predicate(capture_info.node).then_some(capture_info.node));
        let first_node = *filtered_captures.next()?;
        if filtered_captures.next().is_some() {
            return None;
        }
        Some(first_node)
    }

    pub fn get_number_of_query_captures<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        enclosing_node: Node<'enclosing_node>,
    ) -> usize {
        self.get_number_of_filtered_query_captures(query, |_| true, enclosing_node)
    }

    pub fn get_number_of_filtered_query_captures<'query, 'enclosing_node>(
        &self,
        query: impl Into<ParsedOrUnparsedQuery<'query>>,
        mut predicate: impl FnMut(Node) -> bool,
        enclosing_node: Node<'enclosing_node>,
    ) -> usize {
        let query = query
            .into()
            .into_parsed(self.file_run_context.language.language());
        tree_sitter_grep::get_captures_for_enclosing_node(
            self.file_run_context.file_contents,
            &query,
            0,
            None,
            enclosing_node,
        )
        .filter(|capture_info| predicate(capture_info.node))
        .count()
    }

    pub fn pending_fixes(&self) -> Ref<Option<Vec<PendingFix>>> {
        self.pending_fixes.borrow()
    }

    pub fn into_pending_fixes(self) -> Option<Vec<PendingFix>> {
        self.pending_fixes.into_inner()
    }

    pub fn has_named_child_of_kind(&self, node: Node, kind: &str) -> bool {
        let mut cursor = node.walk();
        let ret = node
            .named_children(&mut cursor)
            .any(|child| child.kind() == kind);
        ret
    }

    pub fn get_tokens<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> impl Iterator<Item = Node<'a>> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        let language = self.file_run_context.language;
        get_tokens(node)
            .skip(skip_options.skip())
            .filter(move |node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !language.comment_kinds().contains(node.kind())
                    }
            })
    }

    pub fn get_text_slice(&self, range: ops::Range<usize>) -> Cow<'a, str> {
        get_text_slice(self.file_run_context.file_contents, range)
    }

    pub fn maybe_get_token_after<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Option<Node<'a>> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_tokens_after_node(node)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language
                            .comment_kinds()
                            .contains(node.kind())
                    }
            })
    }

    pub fn get_token_after<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        self.maybe_get_token_after(node, skip_options).unwrap()
    }

    pub fn get_last_token<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_backward_tokens(node)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language
                            .comment_kinds()
                            .contains(node.kind())
                    }
            })
            .unwrap()
    }

    pub fn comments_exist_between(&self, start: Node<'a>, end: Node<'a>) -> bool {
        let comment_kinds = self.file_run_context.language.comment_kinds();
        let end = end.start_byte();
        get_tokens_after_node(start)
            .take_while(|node| node.start_byte() < end)
            .any(|node| comment_kinds.contains(node.kind()))
    }

    pub fn get_first_token<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_tokens(node)
            .skip(skip_options.skip())
            .find(move |node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language
                            .comment_kinds()
                            .contains(node.kind())
                    }
            })
            .unwrap()
    }

    pub fn maybe_get_token_before<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Option<Node<'a>> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        get_tokens_before_node(node)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language
                            .comment_kinds()
                            .contains(node.kind())
                    }
            })
    }

    pub fn get_token_before<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Node<'a> {
        self.maybe_get_token_before(node, skip_options).unwrap()
    }

    pub fn get_tokens_between<TFilter: FnMut(Node) -> bool>(
        &self,
        a: Node<'a>,
        b: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> impl Iterator<Item = Node<'a>> {
        let mut skip_options = skip_options.map(Into::into).unwrap_or_default();
        let b_start = b.start_byte();
        let language = self.file_run_context.language;
        get_tokens_after_node(a)
            .take_while(move |token| token.start_byte() < b_start)
            .skip(skip_options.skip())
            .filter(move |node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !language.comment_kinds().contains(node.kind())
                    }
            })
    }

    pub fn get_comments_after(&self, node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
        let comment_kinds = self.file_run_context.language.comment_kinds();
        get_tokens_after_node(node).take_while(|node| comment_kinds.contains(node.kind()))
    }

    pub fn language(&self) -> SupportedLanguage {
        self.file_run_context.language
    }

    pub fn retrieve<TFromFileRunContext: FromFileRunContext<'a> + for<'d> TidAble<'d>>(
        &self,
    ) -> &TFromFileRunContext {
        self.file_run_context
            .from_file_run_context_instance_provider
            .get::<TFromFileRunContext>(self.file_run_context)
            .unwrap()
    }
}

pub enum ParsedOrUnparsedQuery<'a> {
    Parsed(Query),
    ParsedRef(&'a Query),
    Unparsed(&'a str),
}

impl<'a> ParsedOrUnparsedQuery<'a> {
    pub fn parsed(&self, language: Language) -> MaybeOwned<'_, Query> {
        match self {
            Self::Parsed(query) => query.into(),
            Self::ParsedRef(query) => (*query).into(),
            Self::Unparsed(query_text) => Query::new(language, query_text).unwrap().into(),
        }
    }

    pub fn into_parsed(self, language: Language) -> MaybeOwned<'a, Query> {
        match self {
            Self::Parsed(query) => query.into(),
            Self::ParsedRef(query) => query.into(),
            Self::Unparsed(query_text) => Query::new(language, query_text).unwrap().into(),
        }
    }
}

impl<'a> From<Query> for ParsedOrUnparsedQuery<'a> {
    fn from(value: Query) -> Self {
        Self::Parsed(value)
    }
}

impl<'a> From<&'a Query> for ParsedOrUnparsedQuery<'a> {
    fn from(value: &'a Query) -> Self {
        Self::ParsedRef(value)
    }
}

impl<'a> From<&'a str> for ParsedOrUnparsedQuery<'a> {
    fn from(value: &'a str) -> Self {
        Self::Unparsed(value)
    }
}

pub enum MaybeOwned<'a, T> {
    Owned(T),
    Borrowed(&'a T),
}

impl<'a, T> ops::Deref for MaybeOwned<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            MaybeOwned::Owned(value) => value,
            MaybeOwned::Borrowed(value) => value,
        }
    }
}

impl<'a, T> From<T> for MaybeOwned<'a, T> {
    fn from(value: T) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> From<&'a T> for MaybeOwned<'a, T> {
    fn from(value: &'a T) -> Self {
        Self::Borrowed(value)
    }
}

fn get_node_text<'a>(node: Node, file_contents: RopeOrSlice<'a>) -> Cow<'a, str> {
    get_text_slice(file_contents, node.byte_range())
}

fn get_text_slice(file_contents: RopeOrSlice, range: ops::Range<usize>) -> Cow<'_, str> {
    match file_contents {
        RopeOrSlice::Slice(slice) => std::str::from_utf8(&slice[range]).unwrap().into(),
        RopeOrSlice::Rope(rope) => rope.byte_slice(range).into(),
    }
}
