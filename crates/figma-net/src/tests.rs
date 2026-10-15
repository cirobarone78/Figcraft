use super::*;

#[test]
fn accepts_current_and_legacy_design_links() {
    for product in ["design", "file"] {
        let parsed = parse_design_url(&format!("https://www.figma.com/{product}/SyntheticFile_1-A/Test?node-id=12-34&t=ignored")).unwrap();
        assert_eq!(parsed.file_key, "SyntheticFile_1-A");
        assert_eq!(parsed.node_id.as_deref(), Some("12-34"));
    }
    assert!(parse_design_url("https://figma.com/design/TestFile").is_ok());
    assert!(parse_design_url("https://www.figma.com:443/file/TestFile/Test/").is_ok());
}

#[test]
fn rejects_other_origins_products_and_invalid_keys() {
    for link in [
        "http://www.figma.com/design/TestFile/Test",
        "https://www.figma.com.evil.invalid/design/TestFile/Test",
        "https://evil.invalid/design/TestFile/Test",
        "https://www.figma.com:444/design/TestFile/Test",
        "https://user:secret@www.figma.com/design/TestFile/Test",
        "https://www.figma.com/board/TestFile/Test",
        "https://www.figma.com/slides/TestFile/Test",
        "https://www.figma.com/proto/TestFile/Test",
        "https://www.figma.com/design//Test",
        "https://www.figma.com/design/%2f/Test",
        "https://www.figma.com/design/TestFile/Test/branch/BranchKey",
        "not a URL",
        "https://www.figma.com/design/TestFile/Test\n",
    ] {
        assert!(parse_design_url(link).is_err(), "accepted {link}");
    }
    assert!(parse_design_url(&format!("https://www.figma.com/design/{}/Test", "x".repeat(129))).is_err());
    assert!(parse_design_url(&format!("https://www.figma.com/design/TestFile/Test?node-id={}", "x".repeat(129))).is_err());
}

#[test]
fn command_payload_and_provenance_keep_data_without_credentials() {
    let bundle = ImportBundle {
        file_json: b"{\"document\":{}}".to_vec(),
        variables_json: Some(json!({"meta":{"variables":{},"variableCollections":{}}})),
        images: BTreeMap::from([("imageRef".to_owned(), vec![0, 1, 2, 3])]),
        warnings: vec!["Synthetic warning".to_owned()],
        file_key: "SyntheticFile".to_owned(),
        version: Some("1234".to_owned()),
        name: "Synthetic design".to_owned(),
    };
    let params = bundle.command_params();
    assert_eq!(params["name"], "Synthetic design.json");
    let named = ImportBundle { name: "Specs /".to_owned(), ..bundle.clone() };
    assert_eq!(named.command_params()["name"], "Specs -.json");
    assert_eq!(STANDARD.decode(params["data"].as_str().unwrap()).unwrap(), bundle.file_json);
    assert_eq!(STANDARD.decode(params["images"]["imageRef"].as_str().unwrap()).unwrap(), [0, 1, 2, 3]);
    assert_eq!(params["variables"], bundle.variables_json.clone().unwrap());
    assert!(params.get("token").is_none());
    assert_eq!(bundle.source(), json!({"fileKey":"SyntheticFile","version":"1234","name":"Synthetic design"}));
}

#[test]
fn omitted_variables_stay_omitted() {
    let bundle = ImportBundle {
        file_json: vec![],
        variables_json: None,
        images: BTreeMap::new(),
        warnings: vec![],
        file_key: "TestFile".to_owned(),
        version: None,
        name: "Test".to_owned(),
    };
    assert!(bundle.command_params().get("variables").is_none());
    assert!(bundle.source()["version"].is_null());
}
