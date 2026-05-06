mod config;
mod rules;
mod scanner;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentChanges, Hover, HoverContents, HoverParams, InitializeParams, InitializeResult,
    InitializedParams, MarkupContent, MarkupKind, MessageType,
    OptionalVersionedTextDocumentIdentifier, OneOf, ServerCapabilities, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::config::{ServerConfig, ServerConfigUpdate};
use crate::rules::effective_rules;
use crate::scanner::{contains, scan, to_diagnostics, Finding};

#[derive(Clone, Debug)]
struct DocumentState {
    language_id: String,
    version: i32,
    text: String,
    findings: Vec<Finding>,
    refresh_generation: u64,
}

#[derive(Clone, Debug)]
struct DocumentSnapshot {
    uri: Url,
    language_id: String,
    version: i32,
    text: String,
    refresh_generation: u64,
}

#[derive(Debug)]
struct State {
    initialization_config: RwLock<ServerConfig>,
    workspace_config: RwLock<ServerConfig>,
    documents: RwLock<HashMap<Url, DocumentState>>,
}

impl State {
    fn new(initialization_config: ServerConfig) -> Self {
        Self {
            initialization_config: RwLock::new(initialization_config),
            workspace_config: RwLock::new(ServerConfig::default()),
            documents: RwLock::new(HashMap::new()),
        }
    }

    async fn set_initialization_config(&self, config: ServerConfig) {
        *self.initialization_config.write().await = config;
    }

    async fn current_config(&self) -> ServerConfig {
        let mut config = self.initialization_config.read().await.clone();
        config.merge(self.workspace_config.read().await.clone());
        config
    }

    async fn begin_refresh(&self, uri: &Url) -> Option<DocumentSnapshot> {
        let mut documents = self.documents.write().await;
        let document = documents.get_mut(uri)?;
        document.refresh_generation = document.refresh_generation.saturating_add(1);

        Some(DocumentSnapshot {
            uri: uri.clone(),
            language_id: document.language_id.clone(),
            version: document.version,
            text: document.text.clone(),
            refresh_generation: document.refresh_generation,
        })
    }

    async fn store_findings_if_current(
        &self,
        uri: &Url,
        snapshot: &DocumentSnapshot,
        findings: Vec<Finding>,
    ) -> bool {
        let mut documents = self.documents.write().await;
        let Some(document) = documents.get_mut(uri) else {
            return false;
        };

        if !document.matches(snapshot) {
            return false;
        }

        document.findings = findings;
        true
    }

    async fn clear_findings_if_current(&self, uri: &Url, snapshot: &DocumentSnapshot) -> bool {
        let mut documents = self.documents.write().await;
        let Some(document) = documents.get_mut(uri) else {
            return false;
        };

        if !document.matches(snapshot) {
            return false;
        }

        document.findings.clear();
        true
    }

    async fn update_document(&self, uri: &Url, version: i32, text: String) -> bool {
        let mut documents = self.documents.write().await;
        let Some(document) = documents.get_mut(uri) else {
            return false;
        };

        document.text = text;
        document.version = version;
        document.findings.clear();
        true
    }
}

impl DocumentState {
    fn matches(&self, snapshot: &DocumentSnapshot) -> bool {
        self.version == snapshot.version && self.refresh_generation == snapshot.refresh_generation
    }
}

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Arc<State>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        let config = ServerConfig::from_optional_value(params.initialization_options)
            .map_err(|error| jsonrpc::Error::invalid_params(error.to_string()))?;
        self.state.set_initialization_config(config).await;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                hover_provider: Some(hover_provider()),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    ..WorkspaceServerCapabilities::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "Critters is watching for suspicious Unicode characters.",
            )
            .await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        {
            let mut documents = self.state.documents.write().await;
            documents.insert(
                document.uri.clone(),
                DocumentState {
                    language_id: document.language_id.clone(),
                    version: document.version,
                    text: document.text.clone(),
                    findings: Vec::new(),
                    refresh_generation: 0,
                },
            );
        }
        self.refresh_document(&document.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let new_text = params
            .content_changes
            .into_iter()
            .last()
            .map(|change| change.text);

        if let Some(text) = new_text {
            self.state.update_document(&uri, version, text).await;
            self.refresh_document(&uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = self
            .state
            .documents
            .write()
            .await
            .remove(&uri)
            .map(|document| document.version);
        self.client
            .publish_diagnostics(uri, Vec::new(), version)
            .await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        match ServerConfigUpdate::from_value(params.settings) {
            Ok(Some(update)) => {
                self.state
                    .workspace_config
                    .write()
                    .await
                    .apply_update(update);
                self.refresh_all_documents().await;
            }
            Ok(None) => {
                *self.state.workspace_config.write().await = ServerConfig::default();
                self.refresh_all_documents().await;
            }
            Err(error) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Critters could not parse configuration: {error}"),
                    )
                    .await;
            }
        }
    }

    async fn hover(&self, params: HoverParams) -> jsonrpc::Result<Option<Hover>> {
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        let documents = self.state.documents.read().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };

        let finding = document
            .findings
            .iter()
            .find(|finding| contains(&finding.range, position));

        Ok(finding.map(|finding| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: finding.hover.clone(),
            }),
            range: Some(finding.range),
        }))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> jsonrpc::Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let documents = self.state.documents.read().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };

        let actions = document
            .findings
            .iter()
            .filter(|finding| ranges_overlap(finding.range, params.range))
            .map(|finding| quick_fix_action(&uri, document.version, finding))
            .collect::<Vec<_>>();

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

impl Backend {
    async fn refresh_all_documents(&self) {
        let uris = self
            .state
            .documents
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        for uri in uris {
            self.refresh_document(&uri).await;
        }
    }

    async fn refresh_document(&self, uri: &Url) {
        let Some(snapshot) = self.state.begin_refresh(uri).await else {
            return;
        };

        let config = self.state.current_config().await;
        let findings = match effective_rules(&config, &snapshot.language_id)
            .map(|rules| scan(&snapshot.text, &rules, config.max_diagnostics_per_document))
        {
            Ok(findings) => findings,
            Err(error) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!(
                            "Critters failed to build rules for {}: {error}",
                            snapshot.language_id
                        ),
                    )
                    .await;

                if self.state.clear_findings_if_current(uri, &snapshot).await {
                    self.client
                        .publish_diagnostics(
                            snapshot.uri.clone(),
                            Vec::new(),
                            Some(snapshot.version),
                        )
                        .await;
                }
                return;
            }
        };

        let diagnostics = to_diagnostics(&findings);
        if self
            .state
            .store_findings_if_current(uri, &snapshot, findings)
            .await
        {
            self.client
                .publish_diagnostics(snapshot.uri.clone(), diagnostics, Some(snapshot.version))
                .await;
        }
    }
}

fn hover_provider() -> tower_lsp::lsp_types::HoverProviderCapability {
    tower_lsp::lsp_types::HoverProviderCapability::Simple(true)
}

fn ranges_overlap(left: tower_lsp::lsp_types::Range, right: tower_lsp::lsp_types::Range) -> bool {
    compare_position(left.start, right.end) <= 0 && compare_position(right.start, left.end) <= 0
}

fn compare_position(
    left: tower_lsp::lsp_types::Position,
    right: tower_lsp::lsp_types::Position,
) -> i8 {
    match (
        left.line.cmp(&right.line),
        left.character.cmp(&right.character),
    ) {
        (std::cmp::Ordering::Less, _) => -1,
        (std::cmp::Ordering::Greater, _) => 1,
        (_, std::cmp::Ordering::Less) => -1,
        (_, std::cmp::Ordering::Greater) => 1,
        _ => 0,
    }
}

fn quick_fix_action(uri: &Url, version: i32, finding: &Finding) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: finding.fix_title.clone(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: Some(version),
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: finding.range,
                    new_text: finding.replacement.clone(),
                })],
            }])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        state: Arc::new(State::new(ServerConfig::default())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{quick_fix_action, DocumentState, State};
    use crate::config::{RuleConfig, ServerConfig, Severity};
    use crate::scanner::Finding;
    use tower_lsp::lsp_types::{
        CodeActionOrCommand, DocumentChanges, Position, Range, TextEdit, Url,
    };

    fn sample_finding(message: &str) -> Finding {
        Finding {
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 1),
            },
            severity: Severity::Warning,
            message: message.to_string(),
            hover: message.to_string(),
            fix_title: "Remove suspicious Unicode characters".to_string(),
            replacement: String::new(),
        }
    }

    #[tokio::test]
    async fn initialization_configuration_survives_workspace_updates() {
        let mut init_rules = BTreeMap::new();
        init_rules.insert(
            "00A0".to_string(),
            RuleConfig {
                severity: Some(Severity::Warning),
                ..RuleConfig::default()
            },
        );

        let state = State::new(ServerConfig::default());
        state
            .set_initialization_config(ServerConfig {
                max_diagnostics_per_document: 500,
                rules: init_rules,
                language_overrides: BTreeMap::new(),
            })
            .await;

        *state.workspace_config.write().await = ServerConfig {
            max_diagnostics_per_document: 25,
            ..ServerConfig::default()
        };

        let merged = state.current_config().await;
        assert_eq!(merged.max_diagnostics_per_document, 25);
        assert!(merged.rules.contains_key("00A0"));
    }

    #[tokio::test]
    async fn stale_refresh_results_are_discarded() {
        let state = State::new(Default::default());
        let uri = Url::parse("file:///tmp/critters.rs").expect("valid file uri");

        state.documents.write().await.insert(
            uri.clone(),
            DocumentState {
                language_id: "rust".to_string(),
                version: 1,
                text: "old".to_string(),
                findings: vec![sample_finding("old")],
                refresh_generation: 0,
            },
        );

        let older_snapshot = state
            .begin_refresh(&uri)
            .await
            .expect("older snapshot to exist");

        {
            let mut documents = state.documents.write().await;
            let document = documents.get_mut(&uri).expect("document to exist");
            document.text = "new".to_string();
            document.version = 2;
        }

        let newer_snapshot = state
            .begin_refresh(&uri)
            .await
            .expect("newer snapshot to exist");

        assert!(
            !state
                .store_findings_if_current(&uri, &older_snapshot, vec![sample_finding("stale")])
                .await
        );
        assert!(
            state
                .store_findings_if_current(&uri, &newer_snapshot, vec![sample_finding("fresh")])
                .await
        );

        let documents = state.documents.read().await;
        let findings = &documents.get(&uri).expect("document to exist").findings;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].message, "fresh");
    }

    #[tokio::test]
    async fn current_refresh_failures_clear_cached_findings() {
        let state = State::new(Default::default());
        let uri = Url::parse("file:///tmp/critters.rs").expect("valid file uri");

        state.documents.write().await.insert(
            uri.clone(),
            DocumentState {
                language_id: "rust".to_string(),
                version: 1,
                text: "text".to_string(),
                findings: vec![sample_finding("stale")],
                refresh_generation: 0,
            },
        );

        let snapshot = state.begin_refresh(&uri).await.expect("snapshot to exist");
        assert!(state.clear_findings_if_current(&uri, &snapshot).await);

        let documents = state.documents.read().await;
        assert!(documents
            .get(&uri)
            .expect("document to exist")
            .findings
            .is_empty());
    }

    #[tokio::test]
    async fn document_updates_clear_cached_findings() {
        let state = State::new(Default::default());
        let uri = Url::parse("file:///tmp/critters.rs").expect("valid file uri");

        state.documents.write().await.insert(
            uri.clone(),
            DocumentState {
                language_id: "rust".to_string(),
                version: 1,
                text: "old".to_string(),
                findings: vec![sample_finding("stale")],
                refresh_generation: 0,
            },
        );

        assert!(state.update_document(&uri, 2, "new".to_string()).await);

        let documents = state.documents.read().await;
        let document = documents.get(&uri).expect("document to exist");
        assert_eq!(document.version, 2);
        assert_eq!(document.text, "new");
        assert!(document.findings.is_empty());
    }

    #[test]
    fn quick_fix_actions_are_versioned() {
        let uri = Url::parse("file:///tmp/critters.rs").expect("valid file uri");
        let finding = sample_finding("stale");

        let action = quick_fix_action(&uri, 7, &finding);
        let CodeActionOrCommand::CodeAction(action) = action else {
            panic!("expected code action");
        };

        let edit = action.edit.expect("workspace edit to be present");
        assert!(edit.changes.is_none());

        let Some(DocumentChanges::Edits(edits)) = edit.document_changes else {
            panic!("expected versioned document edits");
        };

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].text_document.uri, uri);
        assert_eq!(edits[0].text_document.version, Some(7));
        assert_eq!(edits[0].edits.len(), 1);

        let tower_lsp::lsp_types::OneOf::Left(TextEdit { range, new_text }) = &edits[0].edits[0]
        else {
            panic!("expected plain text edit");
        };

        assert_eq!(*range, finding.range);
        assert_eq!(new_text, &finding.replacement);
    }
}
