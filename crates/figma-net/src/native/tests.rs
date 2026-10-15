use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Cursor;

use serde_json::json;

use super::*;

const LINK: &str = "https://www.figma.com/design/SyntheticFile/Test";
const TOKEN: &str = "synthetic-token-never-real";
const IMAGE_URL: &str = "https://s3-alpha-sig.figma.com/img/synthetic/ref?Expires=123";

struct MockTransport {
    responses: RefCell<VecDeque<Response>>,
    calls: RefCell<Vec<(String, bool)>>,
}

impl MockTransport {
    fn new(responses: impl IntoIterator<Item = Response>) -> Self {
        Self { responses: RefCell::new(responses.into_iter().collect()), calls: RefCell::new(vec![]) }
    }
}

impl Transport for MockTransport {
    fn get(&self, url: &Url, token: Option<&HeaderValue>, _timeout: Duration) -> Result<Response, ImportError> {
        if let Some(token) = token {
            assert!(token.is_sensitive());
            assert!(is_api_url(url));
        } else {
            assert!(validate_image_url(url).is_ok());
        }
        self.calls.borrow_mut().push((url.as_str().to_owned(), token.is_some()));
        self.responses.borrow_mut().pop_front().ok_or(ImportError::InvalidResponse("synthetic response queue is empty"))
    }
}

fn bytes_response(status: u16, body: &[u8]) -> Response {
    Response { status, content_length: Some(body.len() as u64), retry_after: None, location: None, body: Box::new(Cursor::new(body.to_vec())) }
}

fn json_response(value: Value) -> Response {
    bytes_response(200, &serde_json::to_vec(&value).unwrap())
}

fn file(images: bool) -> Value {
    json!({
        "name":"Synthetic design", "editorType":"figma", "version":"1234",
        "document":{"type":"DOCUMENT","children":[{"type":"CANVAS","children":[
            {"type":"RECTANGLE","fills":if images {json!([{"type":"IMAGE","imageRef":"imageA"}])} else {json!([])}}
        ]}]}
    })
}

fn variables() -> Response {
    json_response(json!({"meta":{"variables":{},"variableCollections":{}}}))
}

fn image_map(url: &str) -> Response {
    json_response(json!({"meta":{"images":{"imageA":url}}}))
}

fn import(transport: &impl Transport) -> Result<ImportBundle, ImportError> {
    import_with(transport, ImportLimits::default(), LINK, TOKEN, &AtomicBool::new(false), |_| {})
}

#[test]
fn downloads_file_variables_and_original_image_bytes() {
    let mock = MockTransport::new([json_response(file(true)), variables(), image_map(IMAGE_URL), bytes_response(200, b"synthetic-image")]);
    let bundle = import(&mock).unwrap();
    assert_eq!(bundle.images["imageA"], b"synthetic-image");
    assert_eq!(bundle.name, "Synthetic design");
    assert_eq!(bundle.version.as_deref(), Some("1234"));
    assert!(bundle.variables_json.is_some());
    assert!(bundle.warnings.is_empty());
    let calls = mock.calls.borrow();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].0, "https://api.figma.com/v1/files/SyntheticFile?geometry=paths");
    assert!(calls[0].1 && calls[1].1 && calls[2].1);
    assert!(!calls[3].1);
}

#[test]
fn optional_variables_failure_keeps_document_and_never_echoes_error_body() {
    let mock = MockTransport::new([json_response(file(false)), bytes_response(403, TOKEN.as_bytes())]);
    let bundle = import(&mock).unwrap();
    assert!(bundle.variables_json.is_none());
    assert_eq!(bundle.warnings.len(), 1);
    assert!(bundle.warnings[0].contains("Enterprise"));
    assert!(!bundle.warnings[0].contains(TOKEN));
    assert_eq!(mock.calls.borrow().len(), 2);
}

#[test]
fn missing_image_link_keeps_placeholder_and_warning() {
    let mock = MockTransport::new([json_response(file(true)), variables(), json_response(json!({"meta":{"images":{"imageA":null}}}))]);
    let bundle = import(&mock).unwrap();
    assert!(bundle.images.is_empty());
    assert_eq!(bundle.warnings.len(), 1);
    assert_eq!(mock.calls.borrow().len(), 3);
}

#[test]
fn malformed_optional_data_is_a_warning() {
    let mock = MockTransport::new([json_response(file(true)), bytes_response(200, b"[]"), bytes_response(200, b"{broken")]);
    let bundle = import(&mock).unwrap();
    assert!(bundle.variables_json.is_none());
    assert!(bundle.images.is_empty());
    assert_eq!(bundle.warnings.len(), 2);
}

#[test]
fn malformed_or_non_design_files_are_rejected() {
    for response in
        [bytes_response(200, b"[]"), bytes_response(200, b"{broken"), json_response(json!({"document":{"type":"DOCUMENT"},"editorType":"figjam"}))]
    {
        let mock = MockTransport::new([response]);
        assert!(matches!(import(&mock), Err(ImportError::InvalidResponse(_))));
        assert_eq!(mock.calls.borrow().len(), 1);
    }
    // Nesting past MAX_JSON_NESTING is refused before parsing (no unbounded recursion); the depth
    // of real files the importer follows parses.
    for levels in [MAX_JSON_NESTING + 1, 5000] {
        let deep = format!("{}0{}", "[".repeat(levels), "]".repeat(levels));
        assert!(parse_json(deep.as_bytes(), "synthetic JSON").is_err());
    }
    let nested = format!("{}0{}", "[".repeat(MAX_JSON_NESTING), "]".repeat(MAX_JSON_NESTING));
    assert!(parse_json(nested.as_bytes(), "synthetic JSON").is_ok());
    assert_eq!(json_nesting(br#"{"a":"[{\"[","b":[[]]}"#), 3);
    assert!(parse_json(b"{} {}", "synthetic JSON").is_err());
}

#[test]
fn file_http_errors_are_readable_and_redacted() {
    for (status, expected) in
        [(401, ImportError::Unauthorized), (403, ImportError::Forbidden), (404, ImportError::NotFound), (500, ImportError::HttpStatus(500))]
    {
        let mock = MockTransport::new([bytes_response(status, TOKEN.as_bytes())]);
        let error = import(&mock).unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error} {error:?}").contains(TOKEN));
        assert_eq!(mock.calls.borrow().len(), 1);
    }
}

#[test]
fn authentication_redirects_are_never_followed() {
    let mut redirect = bytes_response(302, b"");
    redirect.location = Some(IMAGE_URL.to_owned());
    let mock = MockTransport::new([redirect]);
    assert_eq!(import(&mock).unwrap_err(), ImportError::Redirect);
    assert_eq!(mock.calls.borrow().len(), 1);
}

#[test]
fn rejects_unsafe_image_urls_before_network_use() {
    for url in [
        "http://s3-alpha.figma.com/img/a",
        "https://127.0.0.1/img/a",
        "https://s3-alpha.figma.com.evil.invalid/img/a",
        "https://evil-bucket.s3.us-west-2.amazonaws.com/a",
        "https://s3-alpha.figma.com:444/a",
        "https://user:secret@s3-alpha.figma.com/a",
        "https://api.figma.com/v1/files/key",
    ] {
        let mock = MockTransport::new([json_response(file(true)), variables(), image_map(url)]);
        let bundle = import(&mock).unwrap();
        assert!(bundle.images.is_empty());
        assert_eq!(mock.calls.borrow().len(), 3);
        assert!(bundle.warnings[0].contains("refused"));
        assert!(!bundle.warnings[0].contains(url));
    }
}

#[test]
fn safe_image_redirects_are_unauthenticated_and_unsafe_ones_stop() {
    let mut redirect = bytes_response(302, b"");
    redirect.location = Some("https://figma-alpha-api.s3.us-west-2.amazonaws.com/image".to_owned());
    let mock = MockTransport::new([json_response(file(true)), variables(), image_map(IMAGE_URL), redirect, bytes_response(200, b"synthetic-image")]);
    assert_eq!(import(&mock).unwrap().images.len(), 1);
    assert!(mock.calls.borrow().iter().skip(3).all(|(_, authenticated)| !authenticated));

    let mut redirect = bytes_response(302, b"");
    redirect.location = Some("https://localhost/secret".to_owned());
    let mock = MockTransport::new([json_response(file(true)), variables(), image_map(IMAGE_URL), redirect]);
    assert!(import(&mock).unwrap().images.is_empty());
    assert_eq!(mock.calls.borrow().len(), 4);
}

#[test]
fn image_redirect_loops_are_bounded() {
    let redirects = (0..=MAX_REDIRECTS).map(|_| {
        let mut response = bytes_response(302, b"");
        response.location = Some(IMAGE_URL.to_owned());
        response
    });
    let mock = MockTransport::new([json_response(file(true)), variables(), image_map(IMAGE_URL)].into_iter().chain(redirects));
    let bundle = import(&mock).unwrap();
    assert!(bundle.images.is_empty());
    assert_eq!(mock.calls.borrow().len(), 3 + MAX_REDIRECTS + 1);
}

#[test]
fn long_rate_limit_waits_are_returned_without_sleep() {
    let mut response = bytes_response(429, TOKEN.as_bytes());
    response.retry_after = Some(24 * 60 * 60);
    let mock = MockTransport::new([response]);
    assert_eq!(import(&mock).unwrap_err(), ImportError::RateLimited { retry_after_seconds: Some(86400) });
    assert_eq!(mock.calls.borrow().len(), 1);
}

#[test]
fn retries_are_bounded_and_can_succeed() {
    let rate_limit = || {
        let mut response = bytes_response(429, b"");
        response.retry_after = Some(0);
        response
    };
    let mock = MockTransport::new([rate_limit(), json_response(file(false)), variables()]);
    assert!(import(&mock).is_ok());
    assert_eq!(mock.calls.borrow().len(), 3);
    let mock = MockTransport::new((0..=MAX_RETRIES).map(|_| rate_limit()));
    assert!(matches!(import(&mock), Err(ImportError::RateLimited { .. })));
    assert_eq!(mock.calls.borrow().len(), MAX_RETRIES + 1);
}

#[test]
fn cancellation_prevents_requests_and_interrupts_rate_limit_waits() {
    let mock = MockTransport::new([]);
    let cancel = AtomicBool::new(true);
    assert_eq!(import_with(&mock, ImportLimits::default(), LINK, TOKEN, &cancel, |_| {}).unwrap_err(), ImportError::Cancelled);
    assert!(mock.calls.borrow().is_empty());

    let mut response = bytes_response(429, b"");
    response.retry_after = Some(1);
    let mock = MockTransport::new([response]);
    let cancel = AtomicBool::new(false);
    let result = import_with(&mock, ImportLimits::default(), LINK, TOKEN, &cancel, |progress| {
        if progress.stage == ImportStage::Retry {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(result.unwrap_err(), ImportError::Cancelled);
    assert_eq!(mock.calls.borrow().len(), 1);
}

#[test]
fn cancellation_during_a_failed_request_wins_over_network_error() {
    struct CancelTransport<'a>(&'a AtomicBool);
    impl Transport for CancelTransport<'_> {
        fn get(&self, _: &Url, _: Option<&HeaderValue>, _: Duration) -> Result<Response, ImportError> {
            self.0.store(true, Ordering::Relaxed);
            Err(ImportError::Network)
        }
    }
    let cancel = AtomicBool::new(false);
    let transport = CancelTransport(&cancel);
    assert_eq!(import_with(&transport, ImportLimits::default(), LINK, TOKEN, &cancel, |_| {}).unwrap_err(), ImportError::Cancelled);
}

#[test]
fn cancellation_during_a_failed_body_read_wins_over_timeout() {
    struct CancelReader(Arc<AtomicBool>);
    impl Read for CancelReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.0.store(true, Ordering::Relaxed);
            Err(io::Error::new(io::ErrorKind::TimedOut, "synthetic timeout"))
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let response =
        Response { status: 200, content_length: None, retry_after: None, location: None, body: Box::new(CancelReader(Arc::clone(&cancel))) };
    let mut budget = Budget::new(ImportLimits::default());
    assert_eq!(read_body(response, 64, "synthetic body", &mut budget, &cancel).unwrap_err(), ImportError::Cancelled);
}

#[test]
fn a_selected_node_is_reported_as_full_file_import() {
    let mock = MockTransport::new([json_response(file(false)), variables()]);
    let bundle = import_with(&mock, ImportLimits::default(), &format!("{LINK}?node-id=1-2"), TOKEN, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(bundle.warnings.iter().any(|warning| warning.contains("entire Figma file")));
}

#[test]
fn content_length_and_streaming_bodies_obey_limits() {
    let limits = ImportLimits { max_file_bytes: 8, max_total_bytes: 16, ..ImportLimits::default() };
    let mut budget = Budget::new(limits);
    let cancel = AtomicBool::new(false);
    assert!(matches!(read_body(bytes_response(200, b"123456789"), 8, "synthetic body", &mut budget, &cancel), Err(ImportError::Limit(_))));
    assert_eq!(budget.remaining, 16); // Rejected Content-Length is not read.

    let mut response = bytes_response(200, b"123456789012");
    response.content_length = None;
    assert!(matches!(read_body(response, 8, "synthetic body", &mut budget, &cancel), Err(ImportError::Limit(_))));
    assert_eq!(budget.remaining, 7); // At most one extra byte detects an oversized stream.
}

#[test]
fn cumulative_budget_is_exhausted_even_when_a_stream_is_rejected() {
    let mut budget = Budget::new(ImportLimits { max_total_bytes: 8, ..ImportLimits::default() });
    let mut response = bytes_response(200, b"123456789012");
    response.content_length = None;
    assert!(read_body(response, 64, "synthetic body", &mut budget, &AtomicBool::new(false)).is_err());
    assert_eq!(budget.remaining, 0);
    let mock = MockTransport::new([]);
    let url = Url::parse("https://api.figma.com/v1/files/test").unwrap();
    assert!(request_with_retry(&mock, &url, Some(&token_header(TOKEN).unwrap()), &mut budget, &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(mock.calls.borrow().is_empty());
}

#[test]
fn image_reference_count_is_bounded_and_bad_references_are_skipped() {
    let file = json!({"fills":[{"type":"IMAGE","imageRef":"a"},{"type":"IMAGE","imageRef":"b"},{"type":"IMAGE","imageRef":"bad/ref"}]});
    let (refs, invalid, too_many) = image_references(&file, 1).unwrap();
    assert_eq!(refs.len(), 1);
    assert!(invalid && too_many);
}

#[test]
fn oversized_reference_stays_a_placeholder_instead_of_failing_engine_import() {
    let mut value = file(true);
    value["document"]["children"][0]["children"][0]["fills"][0]["imageRef"] = json!("a".repeat(129));
    let mock = MockTransport::new([json_response(value), variables()]);
    let bundle = import(&mock).unwrap();
    assert!(bundle.images.is_empty());
    assert!(bundle.warnings.iter().any(|note| note.contains("invalid references")));
    assert_eq!(mock.calls.borrow().len(), 2);
}

#[test]
fn sensitive_token_headers_are_not_visible_in_debug_and_never_attached_to_assets() {
    for token in ["", "space token", "line\nbreak", "\tToken", "ä", " token"] {
        assert_eq!(token_header(token).unwrap_err(), ImportError::InvalidToken);
    }
    let header = token_header(TOKEN).unwrap();
    assert!(header.is_sensitive());
    assert!(!format!("{header:?}").contains(TOKEN));
    let transport = HttpTransport::new().unwrap();
    let api = Url::parse("https://api.figma.com/v1/files/test").unwrap();
    let asset = Url::parse(IMAGE_URL).unwrap();
    let request = transport.request(&api, Some(&header), REQUEST_TIMEOUT).unwrap().build().unwrap();
    assert!(request.headers()["X-Figma-Token"].is_sensitive());
    let request = transport.request(&asset, None, REQUEST_TIMEOUT).unwrap().build().unwrap();
    assert!(!request.headers().contains_key("X-Figma-Token"));
    assert!(transport.request(&asset, Some(&header), REQUEST_TIMEOUT).is_err());
}

#[test]
fn limits_can_only_be_reduced() {
    assert!(validate_limits(ImportLimits::default()).is_ok());
    assert!(validate_limits(ImportLimits { max_images: 4096, ..ImportLimits::default() }).is_err());
    assert!(validate_limits(ImportLimits { max_image_bytes: 0, ..ImportLimits::default() }).is_err());
}

/// A file well under the byte limit whose value count is high (as with `geometry=paths`) still
/// imports: the traversal budget must not reject what the byte budget accepts.
#[test]
fn many_values_within_the_byte_limit_still_import() {
    let node = |i: usize| {
        json!({"id": format!("1:{i}"), "name": "r", "type": "RECTANGLE",
            "relativeTransform": [[1, 0, i], [0, 1, 0]], "size": {"x": 1, "y": 1},
            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}], "strokes": [], "effects": []})
    };
    let children: Vec<Value> = (0..50_000).map(node).collect();
    let file = json!({"name": "Synthetic large", "editorType": "figma", "version": "1",
        "document": {"type": "DOCUMENT", "children": [{"type": "CANVAS", "children": children}]}});
    let bytes = serde_json::to_vec(&file).unwrap();
    assert!(bytes.len() < ImportLimits::default().max_file_bytes);
    let transport = MockTransport::new([bytes_response(200, &bytes), variables()]);
    let result = import(&transport);
    assert!(result.is_ok(), "{}-byte file JSON: {:?}", bytes.len(), result.err());
}
