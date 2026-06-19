use serde_json::{json, Value};

mod support {
    pub mod lsp;
}

use support::lsp::LspClient;

const TEST_URI: &str = "file:///tmp/critters-protocol.rs";

#[test]
fn initialize_reports_phase_one_capabilities() {
    let mut client = LspClient::start();

    let response = client.initialize();
    let capabilities = response
        .pointer("/result/capabilities")
        .expect("initialize result capabilities");

    assert_eq!(
        capabilities
            .pointer("/textDocumentSync")
            .and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        capabilities
            .pointer("/hoverProvider")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        capabilities
            .pointer("/codeActionProvider")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        capabilities
            .pointer("/workspace/workspaceFolders/supported")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        capabilities
            .pointer("/workspace/workspaceFolders/changeNotifications")
            .and_then(Value::as_bool),
        Some(true)
    );

    let shutdown = client.shutdown_and_exit();
    assert!(shutdown.get("result").is_some());
}

#[test]
fn document_actions_match_protocol_contract() {
    let mut client = LspClient::start();
    client.initialize();

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": TEST_URI,
                "languageId": "rust",
                "version": 1,
                "text": "a\u{00A0}b",
            },
        }),
    );

    let diagnostics = client.wait_for_diagnostics(TEST_URI, Some(1));
    let diagnostic = diagnostics
        .pointer("/params/diagnostics/0")
        .expect("first diagnostic");

    assert_eq!(
        diagnostic.pointer("/source").and_then(Value::as_str),
        Some("critters")
    );
    assert_eq!(
        diagnostic.pointer("/severity").and_then(Value::as_i64),
        Some(3)
    );
    assert!(diagnostic
        .pointer("/message")
        .and_then(Value::as_str)
        .expect("diagnostic message")
        .contains("U+00A0"));
    assert_eq!(diagnostic.pointer("/range/start/line"), Some(&json!(0)));
    assert_eq!(
        diagnostic.pointer("/range/start/character"),
        Some(&json!(1))
    );
    assert_eq!(diagnostic.pointer("/range/end/line"), Some(&json!(0)));
    assert_eq!(diagnostic.pointer("/range/end/character"), Some(&json!(2)));

    let hover = client.request(
        "textDocument/hover",
        json!({
            "textDocument": {
                "uri": TEST_URI,
            },
            "position": {
                "line": 0,
                "character": 1,
            },
        }),
    );
    assert!(hover
        .pointer("/result/contents/value")
        .and_then(Value::as_str)
        .expect("hover markdown")
        .contains("U+00A0"));
    assert_eq!(hover.pointer("/result/range/start/line"), Some(&json!(0)));
    assert_eq!(
        hover.pointer("/result/range/start/character"),
        Some(&json!(1))
    );
    assert_eq!(hover.pointer("/result/range/end/line"), Some(&json!(0)));
    assert_eq!(
        hover.pointer("/result/range/end/character"),
        Some(&json!(2))
    );

    let actions = client.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {
                "uri": TEST_URI,
            },
            "range": {
                "start": {
                    "line": 0,
                    "character": 1,
                },
                "end": {
                    "line": 0,
                    "character": 2,
                },
            },
            "context": {
                "diagnostics": [diagnostic],
            },
        }),
    );
    let action = actions.pointer("/result/0").expect("first code action");
    assert_eq!(
        action.pointer("/kind").and_then(Value::as_str),
        Some("quickfix")
    );
    assert_eq!(
        action.pointer("/isPreferred").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        action.pointer("/edit/documentChanges/0/textDocument/uri"),
        Some(&json!(TEST_URI))
    );
    assert_eq!(
        action.pointer("/edit/documentChanges/0/textDocument/version"),
        Some(&json!(1))
    );
    assert_eq!(
        action.pointer("/edit/documentChanges/0/edits/0/newText"),
        Some(&json!(" "))
    );

    client.shutdown_and_exit();
}

#[test]
fn document_refreshes_after_change_config_and_close() {
    let mut client = LspClient::start();
    client.initialize();

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": TEST_URI,
                "languageId": "rust",
                "version": 1,
                "text": "a\u{00A0}b",
            },
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(1)), 1);

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": TEST_URI,
                "version": 2,
            },
            "contentChanges": [
                {
                    "text": "abc",
                },
            ],
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(2)), 0);

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": TEST_URI,
                "version": 3,
            },
            "contentChanges": [
                {
                    "text": "a\u{00A0}b",
                },
            ],
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(3)), 1);

    client.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "critters-lsp": {
                    "rules": {
                        "00A0": {
                            "severity": "none",
                        },
                    },
                },
            },
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(3)), 0);

    client.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "critters-lsp": null,
            },
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(3)), 1);

    client.notify(
        "textDocument/didClose",
        json!({
            "textDocument": {
                "uri": TEST_URI,
            },
        }),
    );
    assert_diagnostic_count(client.wait_for_diagnostics(TEST_URI, Some(3)), 0);

    client.shutdown_and_exit();
}

fn assert_diagnostic_count(message: Value, expected: usize) {
    let diagnostics = message
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .expect("publishDiagnostics diagnostics array");
    assert_eq!(diagnostics.len(), expected);
}
