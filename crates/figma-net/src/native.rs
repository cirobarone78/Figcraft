use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::header::{HeaderValue, LOCATION, RETRY_AFTER};
use serde_json::Value;
use url::Url;

use crate::{ImportBundle, ImportError, ImportLimits, ImportStage, Progress, parse_design_url};

const API_ORIGIN: &str = "https://api.figma.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_RETRY_WAIT: Duration = Duration::from_secs(30);
const MAX_RETRIES: usize = 2;
const MAX_REDIRECTS: usize = 3;
/// Deepest JSON nesting parsed. The importer follows 256 layer levels, each a node object and its
/// `children` array; serde_json's default limit (128) would refuse files it imports.
const MAX_JSON_NESTING: usize = 2 * 256 + 64;
const READ_CHUNK: usize = 64 * 1024;

/// Reusable native HTTPS clients. Run [`Self::import`] on a worker thread. Cancellation is
/// observed between requests/body reads and during retry waits; an in-flight blocking read can
/// take up to the 30-second request timeout to end. No credential is stored in this client.
pub struct FigmaClient {
    transport: HttpTransport,
    limits: ImportLimits,
}

impl FigmaClient {
    pub fn new() -> Result<Self, ImportError> {
        Self::with_limits(ImportLimits::default())
    }

    /// Override limits downward (for constrained hosts); never disables production bounds.
    pub fn with_limits(limits: ImportLimits) -> Result<Self, ImportError> {
        validate_limits(limits)?;
        Ok(Self { transport: HttpTransport::new()?, limits })
    }

    pub fn import(&self, url: &str, token: &str, cancel: &AtomicBool, progress: impl FnMut(Progress)) -> Result<ImportBundle, ImportError> {
        import_with(&self.transport, self.limits, url, token, cancel, progress)
    }
}

fn validate_limits(limits: ImportLimits) -> Result<(), ImportError> {
    let maximum = ImportLimits::default();
    let fields = [
        (limits.max_file_bytes, maximum.max_file_bytes),
        (limits.max_variables_bytes, maximum.max_variables_bytes),
        (limits.max_image_map_bytes, maximum.max_image_map_bytes),
        (limits.max_image_bytes, maximum.max_image_bytes),
        (limits.max_total_bytes, maximum.max_total_bytes),
        (limits.max_images, maximum.max_images),
    ];
    if fields.iter().any(|(value, max)| *value == 0 || value > max) {
        return Err(ImportError::Limit("nonzero download settings within the default maximums"));
    }
    Ok(())
}

// A private test seam. Production endpoints and certificate/redirect policies are not
// configurable; tests use in-memory responses, without weakening HTTP to allow localhost.
trait Transport {
    fn get(&self, url: &Url, token: Option<&HeaderValue>, timeout: Duration) -> Result<Response, ImportError>;
}

struct Response {
    status: u16,
    content_length: Option<u64>,
    retry_after: Option<u64>,
    location: Option<String>,
    body: Box<dyn Read>,
}

struct HttpTransport {
    api: Client,
    images: Client,
}

impl HttpTransport {
    fn new() -> Result<Self, ImportError> {
        // Explicit ring provider: another workspace dependency enabling aws-lc must not make
        // rustls's default-provider selection ambiguous (and panic at client construction).
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| ImportError::ClientInit)?
            .with_root_certificates(rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()))
            .with_no_client_auth();
        let build = || {
            Client::builder()
                .tls_backend_preconfigured(tls.clone())
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(REQUEST_TIMEOUT)
                .user_agent(concat!("FigCraft/", env!("CARGO_PKG_VERSION"), " FigmaImport"))
                .build()
                .map_err(|_| ImportError::ClientInit)
        };
        Ok(Self { api: build()?, images: build()? })
    }

    fn request(&self, url: &Url, token: Option<&HeaderValue>, timeout: Duration) -> Result<reqwest::blocking::RequestBuilder, ImportError> {
        if let Some(token) = token {
            if !is_api_url(url) {
                return Err(ImportError::InvalidUrl("authentication is only allowed on the Figma API origin"));
            }
            Ok(self.api.get(url.clone()).header("X-Figma-Token", token.clone()).timeout(timeout))
        } else {
            validate_image_url(url)?;
            Ok(self.images.get(url.clone()).timeout(timeout))
        }
    }
}

impl Transport for HttpTransport {
    fn get(&self, url: &Url, token: Option<&HeaderValue>, timeout: Duration) -> Result<Response, ImportError> {
        let response = self.request(url, token, timeout)?.send().map_err(http_error)?;
        Ok(Response {
            status: response.status().as_u16(),
            content_length: response.content_length(),
            retry_after: response.headers().get(RETRY_AFTER).and_then(|header| header.to_str().ok()).and_then(|value| value.trim().parse().ok()),
            location: response.headers().get(LOCATION).and_then(|header| header.to_str().ok()).map(str::to_owned),
            body: Box::new(response),
        })
    }
}

fn http_error(error: reqwest::Error) -> ImportError {
    if error.is_timeout() { ImportError::Timeout } else { ImportError::Network }
}

fn read_error(error: io::Error) -> ImportError {
    if error.kind() == io::ErrorKind::TimedOut
        || error.get_ref().and_then(|error| error.downcast_ref::<reqwest::Error>()).is_some_and(reqwest::Error::is_timeout)
    {
        ImportError::Timeout
    } else {
        ImportError::Network
    }
}

fn token_header(token: &str) -> Result<HeaderValue, ImportError> {
    // Do not silently trim a pasted token: hidden whitespace often means a bad credential.
    if token.is_empty() || token.len() > 4096 || token.bytes().any(|byte| !byte.is_ascii_graphic()) {
        return Err(ImportError::InvalidToken);
    }
    let mut header = HeaderValue::from_str(token).map_err(|_| ImportError::InvalidToken)?;
    header.set_sensitive(true);
    Ok(header)
}

fn is_api_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("api.figma.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn validate_image_url(url: &Url) -> Result<(), ImportError> {
    // Only known Figma image origins, not arbitrary *.amazonaws.com or user-controlled S3
    // buckets. If Figma changes its CDN, the image is skipped with a warning until reviewed.
    let allowed_host = matches!(
        url.host_str(),
        Some(
            "s3-alpha.figma.com"
                | "s3-alpha-sig.figma.com"
                | "figma-alpha-api.s3.amazonaws.com"
                | "figma-alpha-api.s3.us-west-2.amazonaws.com"
                | "figma-alpha-api.s3-us-west-2.amazonaws.com"
        )
    );
    if url.scheme() != "https"
        || !allowed_host
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str().len() > 8192
    {
        return Err(ImportError::UnsafeImageUrl);
    }
    Ok(())
}

struct Budget {
    started: Instant,
    remaining: usize,
    retry_wait: Duration,
}

impl Budget {
    fn new(limits: ImportLimits) -> Self {
        Self { started: Instant::now(), remaining: limits.max_total_bytes, retry_wait: Duration::ZERO }
    }

    fn check(&self, cancel: &AtomicBool) -> Result<(), ImportError> {
        if cancel.load(Ordering::Relaxed) {
            Err(ImportError::Cancelled)
        } else if self.started.elapsed() >= IMPORT_TIMEOUT {
            Err(ImportError::Deadline)
        } else {
            Ok(())
        }
    }

    fn timeout(&self) -> Duration {
        REQUEST_TIMEOUT.min(IMPORT_TIMEOUT.saturating_sub(self.started.elapsed()))
    }

    fn charge(&mut self, amount: usize) -> Result<(), ImportError> {
        if amount > self.remaining {
            self.remaining = 0;
            return Err(ImportError::Limit("total downloaded bytes"));
        }
        self.remaining -= amount;
        Ok(())
    }
}

fn import_with(
    transport: &impl Transport,
    limits: ImportLimits,
    url: &str,
    token: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<ImportBundle, ImportError> {
    let mut budget = Budget::new(limits);
    budget.check(cancel)?;
    let design = parse_design_url(url)?;
    let token = token_header(token)?;
    let mut warnings = Vec::new();
    if design.node_id.is_some() {
        warnings.push("The link selects a node; the entire Figma file was imported.".to_owned());
    }

    report(&mut progress, ImportStage::File, 0, 1);
    let file_url = api_url(&design.file_key, "", true)?;
    let response = request_with_retry(transport, &file_url, Some(&token), &mut budget, cancel, &mut progress)?;
    let file_json = read_body(response, limits.max_file_bytes, "file JSON bytes", &mut budget, cancel)?;
    let file: Value = parse_json(&file_json, "the file JSON is malformed")?;
    if file.get("editorType").and_then(Value::as_str).is_some_and(|editor| editor != "figma") {
        return Err(ImportError::InvalidResponse("the linked file is not a Figma Design file"));
    }
    if file.get("document").and_then(|doc| doc.get("type")).and_then(Value::as_str) != Some("DOCUMENT") {
        return Err(ImportError::InvalidResponse("the file has no Design document root"));
    }
    let name = file.get("name").and_then(Value::as_str).map(clean_name).filter(|name| !name.is_empty()).unwrap_or_else(|| "Figma import".to_owned());
    let version =
        file.get("version").and_then(Value::as_str).filter(|value| value.len() <= 256 && !value.chars().any(char::is_control)).map(str::to_owned);
    let (references, bad_references, too_many_images) = image_references(&file, limits.max_images)?;
    if bad_references {
        warnings.push("Some image fills had invalid references and were left as placeholders.".to_owned());
    }
    if too_many_images {
        warnings.push(format!("Only the first {} distinct image fills were downloaded; further images are placeholders.", limits.max_images));
    }
    drop(file);
    report(&mut progress, ImportStage::File, 1, 1);

    report(&mut progress, ImportStage::Variables, 0, 1);
    let variables = (|| {
        let url = api_url(&design.file_key, "/variables/local", false)?;
        let response = request_with_retry(transport, &url, Some(&token), &mut budget, cancel, &mut progress)?;
        let bytes = read_body(response, limits.max_variables_bytes, "variable JSON bytes", &mut budget, cancel)?;
        let variables = parse_json(&bytes, "the variables JSON is malformed")?;
        if variables.get("meta").and_then(|meta| meta.get("variables")).and_then(Value::as_object).is_none()
            || variables.get("meta").and_then(|meta| meta.get("variableCollections")).and_then(Value::as_object).is_none()
        {
            return Err(ImportError::InvalidResponse("the variables response has no variables/collections map"));
        }
        Ok(variables)
    })();
    let variables_json = match variables {
        Ok(variables) => Some(variables),
        Err(error @ (ImportError::Cancelled | ImportError::Deadline)) => return Err(error),
        Err(error) => {
            warnings.push(format!("Variables were not downloaded ({error}). Figma's Variables REST API requires eligible Enterprise access and file_variables:read; layer values are still imported."));
            None
        }
    };
    report(&mut progress, ImportStage::Variables, 1, 1);

    let mut images = BTreeMap::new();
    report(&mut progress, ImportStage::Images, 0, references.len());
    if !references.is_empty() {
        let image_map = (|| {
            let url = api_url(&design.file_key, "/images", false)?;
            let response = request_with_retry(transport, &url, Some(&token), &mut budget, cancel, &mut progress)?;
            let bytes = read_body(response, limits.max_image_map_bytes, "image download map bytes", &mut budget, cancel)?;
            let map = parse_json(&bytes, "the image download map is malformed")?;
            let map = map
                .get("meta")
                .and_then(|meta| meta.get("images"))
                .and_then(Value::as_object)
                .ok_or(ImportError::InvalidResponse("the image response has no image map"))?;
            Ok(map.clone())
        })();
        match image_map {
            Ok(map) => {
                for (index, reference) in references.iter().enumerate() {
                    budget.check(cancel)?;
                    if budget.remaining == 0 {
                        warnings.push("The total download budget was reached; remaining images are placeholders.".to_owned());
                        break;
                    }
                    let image = (|| {
                        let raw_url = map
                            .get(reference)
                            .and_then(Value::as_str)
                            .ok_or(ImportError::InvalidResponse("a referenced image has no download URL"))?;
                        if raw_url.len() > 8192 || raw_url.chars().any(char::is_control) {
                            return Err(ImportError::UnsafeImageUrl);
                        }
                        let url = Url::parse(raw_url).map_err(|_| ImportError::UnsafeImageUrl)?;
                        let response = download_image(transport, url, &mut budget, cancel, &mut progress)?;
                        let bytes = read_body(response, limits.max_image_bytes, "individual image bytes", &mut budget, cancel)?;
                        if bytes.is_empty() {
                            return Err(ImportError::InvalidResponse("an image download was empty"));
                        }
                        Ok(bytes)
                    })();
                    match image {
                        Ok(bytes) => {
                            images.insert(reference.clone(), bytes);
                        }
                        Err(error @ (ImportError::Cancelled | ImportError::Deadline)) => return Err(error),
                        Err(error) => warnings.push(format!("Image fill {} was left as a placeholder: {error}", index + 1)),
                    }
                    report(&mut progress, ImportStage::Images, index + 1, references.len());
                }
            }
            Err(error @ (ImportError::Cancelled | ImportError::Deadline)) => return Err(error),
            Err(error) => warnings.push(format!("Image download links were unavailable ({error}); image fills are placeholders.")),
        }
    }
    budget.check(cancel)?;
    Ok(ImportBundle { file_json, variables_json, images, warnings, file_key: design.file_key, version, name })
}

fn report(progress: &mut impl FnMut(Progress), stage: ImportStage, completed: usize, total: usize) {
    progress(Progress { stage, completed, total });
}

fn clean_name(name: &str) -> String {
    name.chars().filter(|character| !character.is_control()).take(200).collect::<String>().trim().to_owned()
}

fn api_url(key: &str, suffix: &str, geometry: bool) -> Result<Url, ImportError> {
    let mut url =
        Url::parse(&format!("{API_ORIGIN}/v1/files/{key}{suffix}")).map_err(|_| ImportError::InvalidUrl("could not form a Figma API request"))?;
    if geometry {
        url.set_query(Some("geometry=paths"));
    }
    Ok(url)
}

fn parse_json(bytes: &[u8], message: &'static str) -> Result<Value, ImportError> {
    // The nesting is bounded here, before parsing, instead of by serde_json's recursion limit.
    if json_nesting(bytes) > MAX_JSON_NESTING {
        return Err(ImportError::InvalidResponse(message));
    }
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    parser.disable_recursion_limit();
    let mut values = parser.into_iter::<Value>();
    match (values.next(), values.next()) {
        (Some(Ok(value)), None) => Ok(value),
        _ => Err(ImportError::InvalidResponse(message)),
    }
}

/// The deepest `[`/`{` nesting of JSON text, outside strings (without validating it).
fn json_nesting(bytes: &[u8]) -> usize {
    let (mut depth, mut deepest, mut in_string, mut escaped) = (0_usize, 0_usize, false, false);
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// Image fill references, walking the parsed file without recursion. The file's byte limit
/// already bounds how many values it holds; only containers wait on the stack.
fn image_references(file: &Value, max: usize) -> Result<(BTreeSet<String>, bool, bool), ImportError> {
    let mut references = BTreeSet::new();
    let mut stack = vec![file];
    let mut invalid = false;
    let mut too_many = false;
    let container = |value: &&Value| value.is_object() || value.is_array();
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("IMAGE") {
                    if let Some(reference) = object.get("imageRef").and_then(Value::as_str).filter(|reference| {
                        !reference.is_empty()
                            && reference.len() <= 128
                            && reference.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    }) {
                        if references.len() < max || references.contains(reference) {
                            references.insert(reference.to_owned());
                        } else {
                            too_many = true;
                        }
                    } else {
                        invalid = true;
                    }
                }
                stack.try_reserve(object.len()).map_err(|_| ImportError::Limit("JSON traversal memory"))?;
                stack.extend(object.values().filter(container));
            }
            Value::Array(array) => {
                stack.try_reserve(array.len()).map_err(|_| ImportError::Limit("JSON traversal memory"))?;
                stack.extend(array.iter().filter(container));
            }
            _ => {}
        }
    }
    Ok((references, invalid, too_many))
}

fn request_with_retry(
    transport: &impl Transport,
    url: &Url,
    token: Option<&HeaderValue>,
    budget: &mut Budget,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Progress),
) -> Result<Response, ImportError> {
    for attempt in 0..=MAX_RETRIES {
        budget.check(cancel)?;
        if budget.remaining == 0 {
            return Err(ImportError::Limit("total downloaded bytes"));
        }
        let response = transport.get(url, token, budget.timeout());
        budget.check(cancel)?;
        let response = response?;
        if response.status == 429 {
            let error = ImportError::RateLimited { retry_after_seconds: response.retry_after };
            let delay = Duration::from_secs(response.retry_after.unwrap_or(1));
            if attempt == MAX_RETRIES || delay > MAX_RETRY_WAIT.saturating_sub(budget.retry_wait) {
                return Err(error);
            }
            drop(response);
            wait_retry(delay, budget, cancel, progress)?;
            continue;
        }
        return match response.status {
            200..=299 => Ok(response),
            300..=399 if token.is_none() => Ok(response),
            300..=399 => Err(ImportError::Redirect),
            401 => Err(ImportError::Unauthorized),
            403 => Err(ImportError::Forbidden),
            404 => Err(ImportError::NotFound),
            status => Err(ImportError::HttpStatus(status)),
        };
    }
    Err(ImportError::RateLimited { retry_after_seconds: None })
}

fn wait_retry(delay: Duration, budget: &mut Budget, cancel: &AtomicBool, progress: &mut impl FnMut(Progress)) -> Result<(), ImportError> {
    let start = Instant::now();
    budget.retry_wait += delay;
    report(progress, ImportStage::Retry, 0, delay.as_secs() as usize);
    while start.elapsed() < delay {
        budget.check(cancel)?;
        std::thread::sleep(Duration::from_millis(100).min(delay.saturating_sub(start.elapsed())));
    }
    budget.check(cancel)?;
    report(progress, ImportStage::Retry, delay.as_secs() as usize, delay.as_secs() as usize);
    Ok(())
}

fn download_image(
    transport: &impl Transport,
    mut url: Url,
    budget: &mut Budget,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Progress),
) -> Result<Response, ImportError> {
    for redirects in 0..=MAX_REDIRECTS {
        validate_image_url(&url)?;
        let response = request_with_retry(transport, &url, None, budget, cancel, progress)?;
        if !(300..=399).contains(&response.status) {
            return Ok(response);
        }
        if redirects == MAX_REDIRECTS {
            return Err(ImportError::Redirect);
        }
        let location = response
            .location
            .as_ref()
            .filter(|location| location.len() <= 8192 && !location.chars().any(char::is_control))
            .ok_or(ImportError::Redirect)?;
        url = url.join(location).map_err(|_| ImportError::Redirect)?;
        // The next iteration checks host, HTTPS, credentials and port before a new request.
    }
    Err(ImportError::Redirect)
}

fn read_body(mut response: Response, max: usize, label: &'static str, budget: &mut Budget, cancel: &AtomicBool) -> Result<Vec<u8>, ImportError> {
    budget.check(cancel)?;
    let ceiling = max.min(budget.remaining);
    if response.content_length.is_some_and(|length| length > ceiling as u64) {
        return Err(ImportError::Limit(label));
    }
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        budget.check(cancel)?;
        // Read at most one extra byte to detect a response exceeding the declared/allowed size.
        let read_limit = READ_CHUNK.min(ceiling.saturating_sub(bytes.len()).saturating_add(1));
        let count = response.body.read(&mut chunk[..read_limit]);
        budget.check(cancel)?;
        let count = count.map_err(read_error)?;
        if count == 0 {
            break;
        }
        budget.charge(count)?;
        if count > ceiling.saturating_sub(bytes.len()) {
            return Err(ImportError::Limit(label));
        }
        bytes.try_reserve(count).map_err(|_| ImportError::Limit("download memory"))?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
