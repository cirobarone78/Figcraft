//! The AI agent (MCP) connection's commands: Help ▸ Connect an AI agent asks the app for its
//! window, `app.mcpStart` asks it to listen (on the saved port unless a valid one is given) and
//! `pref.mcpOnLaunch` keeps the choice. Off by default, on port 7979.

use serde_json::json;

use crate::{DEFAULT_MCP_PORT, Prefs, Session};

#[test]
fn off_by_default_on_the_usual_port() {
    let p = Prefs::default();
    assert_eq!((p.mcp_on_launch, p.mcp_port), (false, DEFAULT_MCP_PORT));
    let saved_before: Prefs = serde_json::from_value(json!({"theme": "dark"})).unwrap();
    assert_eq!((saved_before.mcp_on_launch, saved_before.mcp_port), (false, DEFAULT_MCP_PORT), "older preferences keep the defaults");
}

#[test]
fn the_window_and_turning_it_on_are_requests_to_the_app() {
    let mut s = Session::new();
    s.execute("help.mcp", &json!({})).unwrap();
    assert_eq!(s.ui_requests.last().map(|r| r.kind.as_str()), Some("dialog.mcp"));
    assert_eq!(s.execute("app.mcpStart", &json!({})).unwrap(), json!({"port": DEFAULT_MCP_PORT}));
    let r = s.ui_requests.last().unwrap();
    assert_eq!((r.kind.as_str(), &r.data), ("mcp.start", &json!({"port": DEFAULT_MCP_PORT})));
    assert_eq!(s.execute("app.mcpStart", &json!({"port": 8123})).unwrap()["port"], 8123);
    for junk in [json!(0), json!(70000), json!(-1), json!("x"), json!(null)] {
        assert_eq!(s.execute("app.mcpStart", &json!({"port": junk})).unwrap()["port"], DEFAULT_MCP_PORT, "{junk}");
    }
    s.prefs.mcp_port = 9000;
    assert_eq!(s.execute("app.mcpStart", &json!({})).unwrap()["port"], 9000, "the saved port");
}

#[test]
fn turning_it_on_at_launch_is_a_preference() {
    let mut s = Session::new();
    assert_eq!(s.execute("pref.mcpOnLaunch", &json!({})).unwrap(), json!({"on": true}));
    assert!(s.prefs.mcp_on_launch);
    assert_eq!(s.execute("pref.mcpOnLaunch", &json!({"on": false})).unwrap(), json!({"on": false}));
    assert!(!s.prefs.mcp_on_launch);
}
