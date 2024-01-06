use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    ops,
    path::Path,
    sync::Arc,
};

use better_any::{TidAble, TidExt};
use itertools::Itertools;
use maybe_owned::MaybeOwned;
use tracing::{debug, instrument};
use tree_sitter_grep::{
    streaming_iterator::StreamingIterator,
    tree_sitter::{InputEdit, Range, Tree},
    RopeOrSlice, SupportedLanguage, SupportedLanguageLanguage,
};

mod backward_tokens;
mod count_options;
mod get_tokens;
mod node_parent_cache;
mod provided_types;
mod skip_options;

use backward_tokens::{get_backward_tokens, get_tokens_before_node};
pub use count_options::{CountOptions, CountOptionsBuilder};
use get_tokens::get_tokens_after_node;
pub use get_tokens::{get_tokens, TokenWalker};
pub use node_parent_cache::{
    get_node_parent_cache, NodeParentCache, NodeParentProvider, StandaloneNodeParentProvider,
};
pub use provided_types::{
    FromFileRunContext, FromFileRunContextInstanceProvider,
    FromFileRunContextInstanceProviderFactory, FromFileRunContextProvidedTypes,
    FromFileRunContextProvidedTypesOnceLockStorage,
};
pub use skip_options::{SkipOptions, SkipOptionsBuilder};

use crate::{
    environment::Environment,
    fixing::PendingFix,
    rule::InstantiatedRule,
    text::get_text_slice,
    tree_sitter::{Language, Node, Query},
    violation::{Violation, ViolationWithContext},
    AggregatedQueries, Config, Fixer, FixingForSliceRunContext, SourceTextProvider,
};

#[derive(Copy, Clone)]
pub struct FileRunContext<'a, 'b> {
    pub path: &'a Path,
    pub file_contents: RopeOrSlice<'a>,
    pub tree: &'a Tree,
    pub config: &'a Config,
    pub supported_language_language: SupportedLanguageLanguage,
    pub(crate) aggregated_queries: &'a AggregatedQueries<'a>,
    pub(crate) query: &'a Arc<Query>,
    #[allow(dead_code)]
    pub(crate) instantiated_rules: &'a [InstantiatedRule],
    #[allow(dead_code)]
    changed_ranges: Option<&'a [Range]>,
    from_file_run_context_instance_provider: &'b dyn FromFileRunContextInstanceProvider<'a>,
    pub run_kind: RunKind<'a>,
    pub environment: &'a Environment,
    node_parent_cache: &'b Arc<NodeParentCache<'a>>,
}

impl<'a, 'b> FileRunContext<'a, 'b> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: &'a Path,
        file_contents: impl Into<RopeOrSlice<'a>>,
        tree: &'a Tree,
        config: &'a Config,
        supported_language_language: SupportedLanguageLanguage,
        aggregated_queries: &'a AggregatedQueries,
        query: &'a Arc<Query>,
        instantiated_rules: &'a [InstantiatedRule],
        changed_ranges: Option<&'a [Range]>,
        from_file_run_context_instance_provider: &'b dyn FromFileRunContextInstanceProvider<'a>,
        run_kind: RunKind<'a>,
        environment: &'a Environment,
        node_parent_cache: &'b Arc<NodeParentCache<'a>>,
    ) -> Self {
        let file_contents = file_contents.into();
        Self {
            path,
            file_contents,
            tree,
            config,
            supported_language_language,
            aggregated_queries,
            query,
            instantiated_rules,
            changed_ranges,
            from_file_run_context_instance_provider,
            run_kind,
            environment,
            node_parent_cache,
        }
    }

    pub fn retrieve<TFromFileRunContext: FromFileRunContext<'a> + TidAble<'a>>(
        &self,
    ) -> &TFromFileRunContext {
        self.from_file_run_context_instance_provider
            .get(TFromFileRunContext::id(), *self)
            .unwrap()
            .downcast_ref::<TFromFileRunContext>()
            .unwrap()
    }

    pub fn language(&self) -> SupportedLanguage {
        self.supported_language_language.supported_language()
    }
}

impl<'a> SourceTextProvider<'a> for FileRunContext<'a, '_> {
    fn node_text(&self, node: Node) -> Cow<'a, str> {
        self.file_contents.node_text(node)
    }

    fn slice(&self, range: ops::Range<usize>) -> Cow<'a, str> {
        self.file_contents.slice(range)
    }
}

impl<'a> NodeParentProvider<'a> for FileRunContext<'a, '_> {
    fn maybe_node_parent(&self, node: Node<'a>) -> Option<Node<'a>> {
        self.node_parent_cache.get(&node).copied()
    }

    fn standalone_node_parent_provider(&self) -> StandaloneNodeParentProvider<'a> {
        self.node_parent_cache.clone().into()
    }
}

impl From<FileRunContext<'_, '_>> for SupportedLanguage {
    fn from(value: FileRunContext<'_, '_>) -> Self {
        value.language()
    }
}

pub struct QueryMatchContext<'a, 'b> {
    pub file_run_context: FileRunContext<'a, 'b>,
    pub(crate) rule: &'a InstantiatedRule,
    pending_fixes: RefCell<Option<Vec<PendingFix>>>,
    pub(crate) violations: RefCell<Option<Vec<ViolationWithContext>>>,
}

impl<'a, 'b> QueryMatchContext<'a, 'b> {
    pub fn new(file_run_context: FileRunContext<'a, 'b>, rule: &'a InstantiatedRule) -> Self {
        Self {
            file_run_context,
            rule,
            pending_fixes: Default::default(),
            violations: Default::default(),
        }
    }

    #[instrument(level = "debug", skip(self))]
    pub fn report(&self, violation: Violation) {
        debug!("reporting violation");

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
            }
        }
        let violation = violation.contextualize(self, had_fixes);
        self.violations
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .push(violation);
    }

    pub fn get_node_text(&self, node: Node) -> Cow<'a, str> {
        self.node_text(node)
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
            .into_parsed(self.file_run_context.supported_language_language.language());
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
            .into_parsed(self.file_run_context.supported_language_language.language());
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
        let language = self.file_run_context.language();
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
        get_tokens_after_node(node, self)
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language()
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
        get_backward_tokens(node, self.standalone_node_parent_provider())
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language()
                            .comment_kinds()
                            .contains(node.kind())
                    }
            })
            .unwrap()
    }

    pub fn comments_exist_between(&self, start: Node<'a>, end: Node<'a>) -> bool {
        let comment_kinds = self.file_run_context.language().comment_kinds();
        let end = end.start_byte();
        get_tokens_after_node(start, self)
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
                            .language()
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
        get_tokens_before_node(node, self.standalone_node_parent_provider())
            .skip(skip_options.skip())
            .find(|node| {
                skip_options.filter().map_or(true, |filter| filter(*node))
                    && if skip_options.include_comments() {
                        true
                    } else {
                        !self
                            .file_run_context
                            .language()
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
        let language = self.file_run_context.language();
        get_tokens_after_node(a, &self.standalone_node_parent_provider())
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
        let comment_kinds = self.file_run_context.language().comment_kinds();
        get_tokens_after_node(node, &self.standalone_node_parent_provider())
            .take_while(|node| comment_kinds.contains(node.kind()))
    }

    pub fn language(&self) -> SupportedLanguage {
        self.file_run_context.language()
    }

    pub fn retrieve<TFromFileRunContext: FromFileRunContext<'a> + TidAble<'a>>(
        &self,
    ) -> &TFromFileRunContext {
        self.file_run_context.retrieve::<TFromFileRunContext>()
    }

    pub fn get_comments_before(&self, node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
        let comment_kinds = self.file_run_context.language().comment_kinds();
        get_tokens_before_node(node, self.standalone_node_parent_provider())
            .take_while(|node| comment_kinds.contains(node.kind()))
    }

    pub fn get_comments_inside(&self, node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
        let comment_kinds = self.file_run_context.language().comment_kinds();
        get_tokens(node).filter(|node| comment_kinds.contains(node.kind()))
    }

    pub fn get_first_tokens<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        count_options: Option<impl Into<CountOptions<TFilter>>>,
    ) -> impl Iterator<Item = Node<'a>> {
        let mut count_options = count_options.map(Into::into).unwrap_or_default();
        let language = self.file_run_context.language();
        get_tokens(node)
            .take(count_options.count())
            .filter(move |node| {
                count_options.filter().map_or(true, |filter| filter(*node))
                    && if count_options.include_comments() {
                        true
                    } else {
                        !language.comment_kinds().contains(node.kind())
                    }
            })
    }

    pub fn is_space_between(&self, a: Node<'a>, b: Node<'a>) -> bool {
        let a_start = a.start_byte();
        let b_start = b.start_byte();
        let (start, end_byte) = if a_start < b_start {
            (a, b_start)
        } else {
            (b, a_start)
        };
        let mut prev_token = start;
        get_tokens_after_node(start, self)
            .take_while(move |token| token.start_byte() <= end_byte)
            .any(|token| {
                let ret = prev_token.end_byte() < token.start_byte();
                prev_token = token;
                ret
            })
    }

    pub fn get_last_tokens<TFilter: FnMut(Node) -> bool>(
        &self,
        node: Node<'a>,
        count_options: Option<impl Into<CountOptions<TFilter>>>,
    ) -> impl Iterator<Item = Node<'a>> {
        let mut count_options = count_options.map(Into::into).unwrap_or_default();
        let language = self.file_run_context.language();
        get_backward_tokens(node, self.standalone_node_parent_provider())
            .take(count_options.count())
            .filter(move |node| {
                count_options.filter().map_or(true, |filter| filter(*node))
                    && if count_options.include_comments() {
                        true
                    } else {
                        !language.comment_kinds().contains(node.kind())
                    }
            })
            .collect_vec()
            .into_iter()
            .rev()
    }

    pub fn get_first_token_between<TFilter: FnMut(Node) -> bool>(
        &self,
        a: Node<'a>,
        b: Node<'a>,
        skip_options: Option<impl Into<SkipOptions<TFilter>>>,
    ) -> Option<Node<'a>> {
        self.get_tokens_between(a, b, skip_options).next()
    }
}

impl<'a> SourceTextProvider<'a> for QueryMatchContext<'a, '_> {
    fn node_text(&self, node: Node) -> Cow<'a, str> {
        self.file_run_context.node_text(node)
    }

    fn slice(&self, range: ops::Range<usize>) -> Cow<'a, str> {
        self.file_run_context.slice(range)
    }
}

impl<'a> NodeParentProvider<'a> for QueryMatchContext<'a, '_> {
    fn maybe_node_parent(&self, node: Node<'a>) -> Option<Node<'a>> {
        self.file_run_context.maybe_node_parent(node)
    }

    fn standalone_node_parent_provider(&self) -> StandaloneNodeParentProvider<'a> {
        self.file_run_context.standalone_node_parent_provider()
    }
}

impl From<&'_ QueryMatchContext<'_, '_>> for SupportedLanguage {
    fn from(value: &'_ QueryMatchContext<'_, '_>) -> Self {
        value.file_run_context.language()
    }
}

#[derive(Copy, Clone, Debug)]
pub enum RunKind<'a> {
    CommandLineNonfixing,
    CommandLineFixingInitial,
    CommandLineFixingFixingLoop,
    NonfixingForSlice,
    FixingForSliceInitial {
        context: &'a FixingForSliceRunContext,
    },
    FixingForSliceFixingLoop {
        context: &'a FixingForSliceRunContext,
        all_violations_from_last_pass: &'a [ViolationWithContext],
        all_fixes_from_last_pass: &'a [(InputEdit, String)],
    },
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
