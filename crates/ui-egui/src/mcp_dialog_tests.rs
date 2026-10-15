//! Help ▸ Connect an AI agent (MCP): the window opens on request, Turn on asks the platform to
//! listen on the saved port and shows why it couldn't, and the client settings it offers to copy
//! attach `figcraft-cli` to that port.

use std::sync::{Arc, Mutex};

use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::dialogs::mcp_config;
use crate::{FigApp, Services};

/// A platform whose control channel turns on once, at the first port asked, unless `busy`.
struct Channel {
    port: Arc<Mutex<Option<u16>>>,
    busy: bool,
}

impl Services for Channel {
    fn control_port(&self) -> Option<u16> {
        *self.port.lock().unwrap()
    }
    fn control_start(&mut self, port: u16) -> Result<u16, String> {
        if self.busy {
            return Err("address in use".into());
        }
        Ok(*self.port.lock().unwrap().get_or_insert(port))
    }
}

fn app(busy: bool) -> (FigApp, Arc<Mutex<Option<u16>>>) {
    let port = Arc::new(Mutex::new(None));
    (FigApp::new(Session::new(), Box::new(Channel { port: port.clone(), busy })), port)
}

#[test]
fn the_window_opens_and_turn_on_listens_on_the_saved_port() {
    let (mut a, port) = app(false);
    a.run("help.mcp", json!({}));
    assert!(a.ui.mcp && a.ui.mcp_error.is_none());
    a.session.prefs.mcp_port = 8124;
    a.run("app.mcpStart", json!({}));
    assert_eq!(*port.lock().unwrap(), Some(8124));
    assert!(a.ui.mcp_error.is_none());
    a.run("app.mcpStart", json!({"port": 8125}));
    assert_eq!(a.services.control_port(), Some(8124), "on already: one channel per app");
}

#[test]
fn a_port_that_is_taken_says_why() {
    let (mut a, port) = app(true);
    a.run("app.mcpStart", json!({}));
    assert_eq!(*port.lock().unwrap(), None);
    assert_eq!(a.ui.mcp_error.as_deref(), Some("address in use"));
}

#[test]
fn the_client_settings_attach_figcraft_cli_to_the_port() {
    let v: Value = serde_json::from_str(&mcp_config("/opt/figcraft-cli", 7979)).unwrap();
    assert_eq!(v["mcpServers"]["figcraft"], json!({"command": "/opt/figcraft-cli", "args": ["mcp", "--attach", "7979"]}));
}
