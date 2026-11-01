use lsp_types::{
    DocumentSymbolResponse, FoldingRangeProviderCapability, HoverProviderCapability,
    InitializeResult, OneOf, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::{Arc, Mutex};
use tracing::error;

use crate::parser::TraceDocument;

#[derive(Debug, Serialize, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub struct ServerState {
    documents: HashMap<Url, TraceDocument>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }
}

pub fn run_stdio_server() -> io::Result<()> {
    let state = Arc::new(Mutex::new(ServerState::new()));
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    loop {
        let mut content_length: Option<usize> = None;
        let mut header_line = String::new();

        loop {
            header_line.clear();
            let bytes_read = stdin_lock.read_line(&mut header_line)?;
            if bytes_read == 0 {
                return Ok(()); // EOF
            }
            let trimmed = header_line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // End of headers
            }
            if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                if let Ok(len) = len_str.trim().parse::<usize>() {
                    content_length = Some(len);
                }
            }
        }

        let Some(length) = content_length else {
            continue;
        };

        let mut body_buf = vec![0u8; length];
        stdin_lock.read_exact(&mut body_buf)?;

        let request: RpcRequest = match serde_json::from_slice(&body_buf) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse JSON-RPC: {e}");
                continue;
            }
        };

        let response = handle_rpc(request, &state);

        if let Some(resp) = response {
            let resp_bytes = serde_json::to_vec(&resp)?;
            write!(
                stdout_lock,
                "Content-Length: {}\r\n\r\n",
                resp_bytes.len()
            )?;
            stdout_lock.write_all(&resp_bytes)?;
            stdout_lock.flush()?;
        }
    }
}

fn handle_rpc(req: RpcRequest, state: &Arc<Mutex<ServerState>>) -> Option<RpcResponse> {
    let id = req.id;
    let method = req.method.as_str();

    match method {
        "initialize" => {
            let capabilities = ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
                document_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(crate::tokens::server_capabilities()),
                ..Default::default()
            };

            let result = InitializeResult {
                capabilities,
                server_info: Some(ServerInfo {
                    name: "trace-lsp".to_string(),
                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                }),
            };

            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(result).unwrap()),
                error: None,
            })
        }
        "initialized" => None,
        "textDocument/didOpen" => {
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(params_val) {
                    let doc = TraceDocument::parse(&params.text_document.text);
                    let mut lock = state.lock().unwrap();
                    lock.documents.insert(params.text_document.uri, doc);
                }
            }
            None
        }
        "textDocument/didChange" => {
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(params_val) {
                    if let Some(change) = params.content_changes.into_iter().next() {
                        let doc = TraceDocument::parse(&change.text);
                        let mut lock = state.lock().unwrap();
                        lock.documents.insert(params.text_document.uri, doc);
                    }
                }
            }
            None
        }
        "textDocument/didClose" => {
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(params_val) {
                    let mut lock = state.lock().unwrap();
                    lock.documents.remove(&params.text_document.uri);
                }
            }
            None
        }
        "textDocument/documentSymbol" => {
            let started = std::time::Instant::now();
            let mut symbols = Vec::new();
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::DocumentSymbolParams>(params_val) {
                    let uri = params.text_document.uri;
                    let mut lock = state.lock().unwrap();
                    if !lock.documents.contains_key(&uri) {
                        // Zed's outline modal silently refuses to open on an empty
                        // response, so never return [] just because didOpen raced us.
                        if let Some(text) = uri.to_file_path().ok().and_then(|p| std::fs::read_to_string(p).ok()) {
                            eprintln!("trace-lsp: {uri} not open, parsed from disk");
                            lock.documents.insert(uri.clone(), TraceDocument::parse(&text));
                        }
                    }
                    if let Some(doc) = lock.documents.get(&uri) {
                        symbols = doc.to_document_symbols();
                    }
                }
            }
            fn count(s: &[lsp_types::DocumentSymbol]) -> usize {
                s.iter().map(|c| 1 + c.children.as_deref().map_or(0, count)).sum()
            }
            eprintln!(
                "trace-lsp: documentSymbol -> {} symbols in {:.1?}",
                count(&symbols),
                started.elapsed()
            );
            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(DocumentSymbolResponse::Nested(symbols)).unwrap()),
                error: None,
            })
        }
        "textDocument/foldingRange" => {
            let mut ranges = Vec::new();
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::FoldingRangeParams>(params_val) {
                    let lock = state.lock().unwrap();
                    if let Some(doc) = lock.documents.get(&params.text_document.uri) {
                        ranges = doc.to_folding_ranges();
                    }
                }
            }
            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(ranges).unwrap()),
                error: None,
            })
        }
        "textDocument/hover" => {
            let mut hover_res = None;
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::HoverParams>(params_val) {
                    let lock = state.lock().unwrap();
                    let uri = &params.text_document_position_params.text_document.uri;
                    let pos = params.text_document_position_params.position;
                    if let Some(doc) = lock.documents.get(uri) {
                        hover_res = doc.hover_at(pos);
                    }
                }
            }
            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(hover_res).unwrap()),
                error: None,
            })
        }
        "textDocument/definition" => {
            let mut def_res = None;
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::GotoDefinitionParams>(params_val) {
                    let lock = state.lock().unwrap();
                    let uri = &params.text_document_position_params.text_document.uri;
                    let pos = params.text_document_position_params.position;
                    if let Some(doc) = lock.documents.get(uri) {
                        def_res = doc.goto_definition(pos, uri);
                    }
                }
            }
            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(def_res).unwrap()),
                error: None,
            })
        }
        "textDocument/semanticTokens/full" => {
            let mut tokens_res = lsp_types::SemanticTokensResult::Tokens(lsp_types::SemanticTokens {
                result_id: None,
                data: Vec::new(),
            });
            if let Some(params_val) = req.params {
                if let Ok(params) = serde_json::from_value::<lsp_types::SemanticTokensParams>(params_val) {
                    let lock = state.lock().unwrap();
                    if let Some(doc) = lock.documents.get(&params.text_document.uri) {
                        tokens_res = lsp_types::SemanticTokensResult::Tokens(crate::tokens::tokenize_document(&doc.raw_text));
                    }
                }
            }
            Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::to_value(tokens_res).unwrap()),
                error: None,
            })
        }
        "shutdown" => Some(RpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(Value::Null),
            error: None,
        }),
        "exit" => {
            std::process::exit(0);
        }
        _ => {
            // Unhandled request: return method not found error if request had an ID
            if id.is_some() {
                Some(RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(serde_json::json!({
                        "code": -32601,
                        "message": format!("Method not found: {method}")
                    })),
                })
            } else {
                None
            }
        }
    }
}
