mod config;
mod rules;
mod scanner;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::{
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverContents, HoverParams, InitializeParams,
    InitializeResult, InitializedParams, MarkupContent, MarkupKind, MessageType, OneOf,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::config::ServerConfig;
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
        if let Ok(config) = ServerConfig::from_optional_value(params.initialization_options) {
            self.state.set_initialization_config(config).await;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
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
            if let Some(document) = self.state.documents.write().await.get_mut(&uri) {
                document.text = text;
                document.version = version;
            }
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
        match ServerConfig::from_value(params.settings) {
            Ok(config) => {
                *self.state.workspace_config.write().await = config;
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

    use super::{DocumentState, State};
    use crate::config::{RuleConfig, ServerConfig, Severity};
    use crate::scanner::Finding;
    use tower_lsp::lsp_types::{Position, Range, Url};

    fn sample_finding(message: &str) -> Finding {
        Finding {
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 1),
            },
            severity: Severity::Warning,
            message: message.to_string(),
            hover: message.to_string(),
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
}
