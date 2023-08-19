use std::{collections::HashMap, ops, path::Path};

use squalid::EverythingExt;
use tokio::sync::Mutex;
use tower_lsp::{
    jsonrpc::Result,
    lsp_types::{
        Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
        DocumentChanges, ExecuteCommandOptions, ExecuteCommandParams, InitializeParams,
        InitializeResult, InitializedParams, NumberOrString, OneOf,
        OptionalVersionedTextDocumentIdentifier, Position, Range, ServerCapabilities,
        TextDocumentEdit, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
        WorkspaceEdit,
    },
    Client, LanguageServer, LspService, Server,
};
use tree_sitter_grep::{ropey::Rope, RopeOrSlice};

use crate::{
    fixing::{get_newline_offsets_rope_or_slice, AccumulatedEdits},
    tree_sitter::{self, InputEdit, Parser, Point, Tree},
    tree_sitter_grep::{Parseable, SupportedLanguage},
    Args, ArgsBuilder, FixingForSliceRunContext, FixingForSliceRunStatus, MutRopeOrSlice,
    ViolationWithContext,
};

const APPLY_ALL_FIXES_COMMAND: &str = "tree-sitter-lint.applyAllFixes";

pub trait LocalLinter: Send + Sync {
    fn run_for_slice<'a>(
        &self,
        file_contents: impl Into<RopeOrSlice<'a>>,
        tree: Option<Tree>,
        path: impl AsRef<Path>,
        args: Args,
        language: SupportedLanguage,
    ) -> Vec<ViolationWithContext>;

    fn run_fixing_for_slice<'a>(
        &self,
        file_contents: impl Into<MutRopeOrSlice<'a>>,
        tree: Option<Tree>,
        path: impl AsRef<Path>,
        args: Args,
        language: SupportedLanguage,
        context: FixingForSliceRunContext,
    ) -> FixingForSliceRunStatus;
}

#[derive(Debug)]
struct Backend<TLocalLinter> {
    client: Client,
    local_linter: TLocalLinter,
    per_file: Mutex<HashMap<Url, PerFileState>>,
}

impl<TLocalLinter: LocalLinter> Backend<TLocalLinter> {
    pub fn new(client: Client, local_linter: TLocalLinter) -> Self {
        Self {
            client,
            local_linter,
            per_file: Default::default(),
        }
    }

    async fn run_linting_and_report_diagnostics(&self, uri: &Url) {
        let (file_contents, tree) = {
            let per_file = self.per_file.lock().await;
            let per_file_state = per_file.get(uri).unwrap();
            (per_file_state.contents.clone(), per_file_state.tree.clone())
        };
        let violations = self.local_linter.run_for_slice(
            &file_contents,
            Some(tree),
            "dummy_path",
            Default::default(),
            SupportedLanguage::Rust,
        );
        self.client
            .publish_diagnostics(
                uri.clone(),
                violations
                    .into_iter()
                    .map(|violation| Diagnostic {
                        message: violation.message().into_owned(),
                        range: tree_sitter_range_to_lsp_range(&file_contents, violation.range),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String(violation.rule.name.clone())),
                        source: Some("tree-sitter-lint".to_owned()),
                        ..Default::default()
                    })
                    .collect(),
                None,
            )
            .await;
    }

    async fn run_fixing_and_report_fixes(&self, uri: &Url) {
        let (file_contents, tree, edits_since_last_fixing_run, last_fixing_run_violations) = {
            let per_file = self.per_file.lock().await;
            let per_file_state = per_file.get(uri).unwrap();
            (
                per_file_state.contents.clone(),
                per_file_state.tree.clone(),
                match &per_file_state.edits_since_last_fixing_run {
                    AccumulatedEditsOrEntireFileChanged::AccumulatedEdits(accumulated_edits) => {
                        Some(accumulated_edits.clone())
                    }
                    AccumulatedEditsOrEntireFileChanged::EntireFileChanged => None,
                },
                per_file_state.last_fixing_run_violations.clone(),
            )
        };
        let mut cloned_contents = file_contents.clone();
        let FixingForSliceRunStatus {
            edits, violations, ..
        } = self.local_linter.run_fixing_for_slice(
            &mut cloned_contents,
            Some(tree),
            "dummy_path",
            ArgsBuilder::default().fix(true).build().unwrap(),
            SupportedLanguage::Rust,
            FixingForSliceRunContext {
                last_fixing_run_violations,
                edits_since_last_fixing_run,
            },
        );
        self.per_file
            .lock()
            .await
            .get_mut(uri)
            .unwrap()
            .thrush(|per_file_state| {
                per_file_state.last_fixing_run_violations = Some(violations);
                per_file_state.edits_since_last_fixing_run =
                    AccumulatedEditsOrEntireFileChanged::AccumulatedEdits(AccumulatedEdits::new(
                        get_newline_offsets_rope_or_slice(&cloned_contents).collect(),
                    ));
            });
        if let Some(edits) = edits {
            self.client
                .apply_edit(WorkspaceEdit {
                    document_changes: Some(DocumentChanges::Edits(vec![get_text_document_edits(
                        &edits,
                        uri,
                        &cloned_contents,
                    )])),
                    ..Default::default()
                })
                .await
                .unwrap();
        }
    }
}

#[tower_lsp::async_trait]
impl<TLocalLinter: LocalLinter + 'static> LanguageServer for Backend<TLocalLinter> {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![APPLY_ALL_FIXES_COMMAND.to_owned()],
                    work_done_progress_options: Default::default(),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // self.client
        //     .log_message(tower_lsp::lsp_types::MessageType::INFO, "server initialized!")
        //     .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let contents: Rope = (&*params.text_document.text).into();
        let uri = params.text_document.uri.clone();
        let tree = parse_from_scratch(&contents);
        self.per_file.lock().await.insert(
            uri,
            PerFileState {
                tree,
                edits_since_last_fixing_run: AccumulatedEditsOrEntireFileChanged::AccumulatedEdits(
                    AccumulatedEdits::new(get_newline_offsets_rope_or_slice(&contents).collect()),
                ),
                contents,
                last_fixing_run_violations: Default::default(),
            },
        );

        self.run_linting_and_report_diagnostics(&params.text_document.uri)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        {
            // TODO: refine mutex-holding here?
            let mut per_file = self.per_file.lock().await;
            let file_state = per_file
                .get_mut(&params.text_document.uri)
                .expect("Changed file wasn't loaded");
            for content_change in &params.content_changes {
                match content_change.range {
                    Some(range) => {
                        let start_char =
                            lsp_position_to_char_offset(&file_state.contents, range.start);
                        let end_char = lsp_position_to_char_offset(&file_state.contents, range.end);
                        let start_byte = file_state.contents.char_to_byte(start_char);
                        let old_end_byte = file_state.contents.char_to_byte(end_char);
                        file_state.contents.remove(start_char..end_char);
                        file_state.contents.insert(start_char, &content_change.text);

                        let new_end_byte = start_byte + content_change.text.len();
                        let input_edit = InputEdit {
                            start_byte,
                            old_end_byte,
                            new_end_byte,
                            start_position: position_to_point(range.start),
                            old_end_position: position_to_point(range.end),
                            new_end_position: byte_offset_to_point(
                                &file_state.contents,
                                new_end_byte,
                            ),
                        };
                        file_state.tree.edit(&input_edit);
                        file_state.tree = parse(&file_state.contents, Some(&file_state.tree));
                        if let AccumulatedEditsOrEntireFileChanged::AccumulatedEdits(
                            edits_since_last_fixing_run,
                        ) = &mut file_state.edits_since_last_fixing_run
                        {
                            edits_since_last_fixing_run
                                .add_round_of_edits(&[(input_edit, &content_change.text)]);
                        };
                    }
                    None => {
                        file_state.contents = (&*content_change.text).into();
                        file_state.tree = parse_from_scratch(&file_state.contents);
                        file_state.edits_since_last_fixing_run =
                            AccumulatedEditsOrEntireFileChanged::EntireFileChanged;
                    }
                }
            }
        }

        self.run_linting_and_report_diagnostics(&params.text_document.uri)
            .await;
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        match &*params.command {
            APPLY_ALL_FIXES_COMMAND => {
                self.run_fixing_and_report_fixes(&get_uri_from_arguments(&params.arguments))
                    .await;
            }
            command => panic!("Unknown command: {:?}", command),
        }

        Ok(None)
    }
}

#[derive(Debug)]
struct PerFileState {
    contents: Rope,
    tree: Tree,
    edits_since_last_fixing_run: AccumulatedEditsOrEntireFileChanged,
    last_fixing_run_violations: Option<Vec<ViolationWithContext>>,
}

#[derive(Debug)]
enum AccumulatedEditsOrEntireFileChanged {
    AccumulatedEdits(AccumulatedEdits),
    EntireFileChanged,
}

fn parse_from_scratch(contents: &Rope) -> Tree {
    parse(contents, None)
}

fn parse(contents: &Rope, old_tree: Option<&Tree>) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(SupportedLanguage::Rust.language())
        .unwrap();
    contents.parse(&mut parser, old_tree).unwrap()
}

fn lsp_position_to_char_offset(file_contents: &Rope, position: Position) -> usize {
    file_contents.line_to_char(position.line as usize) + position.character as usize
}

fn position_to_point(position: Position) -> Point {
    Point {
        row: position.line as usize,
        column: position.character as usize,
    }
}

fn point_to_position(point: Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

fn byte_offset_to_point(file_contents: &Rope, byte_offset: usize) -> Point {
    let line_num = file_contents.byte_to_line(byte_offset);
    let start_of_line_byte_offset = file_contents.line_to_byte(line_num);
    Point {
        row: line_num,
        column: byte_offset - start_of_line_byte_offset,
    }
}

fn byte_offset_to_position(file_contents: &Rope, byte_offset: usize) -> Position {
    point_to_position(byte_offset_to_point(file_contents, byte_offset))
}

fn byte_offset_range_to_lsp_range(file_contents: &Rope, range: ops::Range<usize>) -> Range {
    Range {
        start: byte_offset_to_position(file_contents, range.start),
        end: byte_offset_to_position(file_contents, range.end),
    }
}

fn tree_sitter_range_to_lsp_range(file_contents: &Rope, range: tree_sitter::Range) -> Range {
    byte_offset_range_to_lsp_range(file_contents, range.start_byte..range.end_byte)
}

fn get_text_document_edits(
    edits: &AccumulatedEdits,
    uri: &Url,
    new_contents: &Rope,
) -> TextDocumentEdit {
    TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: None,
        },
        edits: edits
            .get_old_ranges_and_new_byte_ranges()
            .into_iter()
            .map(|(old_range, new_byte_range)| TextEdit {
                range: Range {
                    start: point_to_position(old_range.start_point),
                    end: point_to_position(old_range.end_point),
                },
                new_text: new_contents.slice(new_byte_range).into(),
            })
            .map(OneOf::Left)
            .collect(),
    }
}

fn get_uri_from_arguments(arguments: &[serde_json::Value]) -> Url {
    if arguments.len() != 1 {
        panic!("Expected to get passed a single file description");
    }
    match &arguments[0] {
        serde_json::Value::Object(map) => map["uri"].as_str().unwrap().try_into().unwrap(),
        _ => panic!("Expected file description to be object"),
    }
}

pub async fn run<TLocalLinter: LocalLinter + 'static>(local_linter: TLocalLinter) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend::new(client, local_linter));
    Server::new(stdin, stdout, socket).serve(service).await;
}
