//! Native Figma import shared by the CLI and MCP tool. Credentials come from the hosting
//! process, never from tool arguments, command journals or document metadata.

use serde_json::Value;

use crate::Backend;

/// Fetch a whole Design file and open it as a new, unsaved, editable document.
/// `FIGCRAFT_FIGMA_TOKEN` must be set in the CLI/MCP process environment. This function blocks;
/// desktop UI uses the same transport on its cancellable worker instead.
#[cfg(not(target_arch = "wasm32"))]
pub fn import_figma(backend: &mut dyn Backend, url: &str) -> Result<Value, String> {
    use std::sync::atomic::AtomicBool;

    use figcraft_figma_net::{FigmaClient, parse_design_url};
    parse_design_url(url).map_err(|e| e.to_string())?;
    let token = std::env::var("FIGCRAFT_FIGMA_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "Set FIGCRAFT_FIGMA_TOKEN in the CLI or MCP process environment with file_content:read permission".to_string())?;
    let bundle = FigmaClient::new().map_err(|e| e.to_string())?.import(url, &token, &AtomicBool::new(false), |_| {}).map_err(|e| e.to_string())?;
    drop(token);
    open_bundle(backend, bundle)
}

#[cfg(not(target_arch = "wasm32"))]
fn open_bundle(backend: &mut dyn Backend, bundle: figcraft_figma_net::ImportBundle) -> Result<Value, String> {
    use serde_json::json;

    let source = bundle.source();
    let params = bundle.command_params();
    let mut warnings = bundle.warnings;
    // An attached app that times out still opens the queued document later: saying so keeps
    // clients from repeating a finished download into a duplicate tab.
    let mut opened = backend.call("engine.execute", json!({"command": "file.open", "params": params})).map_err(|e| {
        if e.contains("timed out") {
            format!("{e}. The download finished and the app may still open the document: check its tabs before importing again")
        } else {
            e
        }
    })?;
    warnings.extend(opened.get("warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
    opened["warnings"] = json!(warnings);
    // Opening has already succeeded. Inspection failure must not turn it into an apparent
    // failed import which clients might repeat and create duplicate tabs.
    let document = backend.call("document.inspect", json!({"depth": 1})).ok();
    Ok(json!({"source": source, "opened": opened, "document": document}))
}

#[cfg(target_arch = "wasm32")]
pub fn import_figma(_: &mut dyn Backend, _: &str) -> Result<Value, String> {
    Err("Figma URL import requires a native CLI/MCP process. Open a local .fig or REST JSON file instead".into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn invalid_link_leaves_session_unchanged() {
        let mut backend = crate::Headless::new();
        let count = backend.session.docs.len();
        assert!(import_figma(&mut backend, "https://example.com/design/abc/Secret").is_err());
        assert_eq!(backend.session.docs.len(), count);
    }

    #[test]
    fn tool_schema_declares_network_without_a_credential_argument() {
        let definitions = crate::tool_definitions();
        let tool = definitions.iter().find(|tool| tool["name"] == "import_figma").unwrap();
        assert_eq!(tool["annotations"]["openWorldHint"], true);
        assert_eq!(tool["inputSchema"]["required"], json!(["url"]));
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(tool["inputSchema"]["properties"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn tool_rejects_credential_arguments_without_echoing_them() {
        let mut backend = crate::Headless::new();
        let result =
            crate::call_tool(&mut backend, "import_figma", &json!({"url": "https://www.figma.com/design/abc/File", "token": "synthetic-secret"}));
        assert!(result.is_error);
        assert!(!result.first_text().contains("synthetic-secret"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn completed_bundle_uses_supported_backend_methods_and_merges_notes() {
        struct Fake {
            calls: Vec<String>,
        }
        impl Backend for Fake {
            fn call(&mut self, method: &str, _: Value) -> Result<Value, String> {
                self.calls.push(method.into());
                match method {
                    "engine.execute" => Ok(json!({"index": 1, "warnings": ["parser note"]})),
                    "document.inspect" => Ok(json!({"name": "Imported design"})),
                    _ => Err("unsupported control method".into()),
                }
            }
            fn has_ui(&self) -> bool {
                false
            }
            fn describe(&self) -> String {
                "synthetic".into()
            }
        }
        let bundle = figcraft_figma_net::ImportBundle {
            file_json: b"{}".to_vec(),
            variables_json: None,
            images: Default::default(),
            warnings: vec!["transport note".into()],
            file_key: "abc".into(),
            version: Some("v1".into()),
            name: "Imported design".into(),
        };
        let mut backend = Fake { calls: Vec::new() };
        let result = open_bundle(&mut backend, bundle).unwrap();
        assert_eq!(backend.calls, ["engine.execute", "document.inspect"]);
        assert_eq!(result["document"]["name"], "Imported design");
        assert_eq!(result["opened"]["warnings"], json!(["transport note", "parser note"]));
        assert_eq!(result["source"]["fileKey"], "abc");
    }
}
