//! Read-only imports through Figma's documented REST API.
//!
//! The portable types and URL parser also build for WASM. The native [`FigmaClient`] (on native
//! targets only) belongs on a worker thread, never the UI thread or an async executor. Tokens
//! are borrowed for one import, are not stored in clients/bundles, and are sent only to the
//! fixed API origin. Conversion into an editable document stays in FigCraft's engine.
//!
//! API contract: <https://developers.figma.com/docs/rest-api/file-endpoints/>,
//! <https://developers.figma.com/docs/rest-api/variables-endpoints/> and
//! <https://developers.figma.com/docs/rest-api/rate-limits/>.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::FigmaClient;

/// The file key and optional selection in a public Figma Design link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignUrl {
    pub file_key: String,
    /// A link selection is recorded, but the importer fetches the entire editable file.
    pub node_id: Option<String>,
}

/// Accept modern `/design/` and older `/file/` links, without opening the URL itself.
/// Other Figma products, custom ports, credentials and lookalike hosts are refused.
pub fn parse_design_url(input: &str) -> Result<DesignUrl, ImportError> {
    if input.len() > 4096 || input.chars().any(char::is_control) {
        return Err(ImportError::InvalidUrl("the link is too long or contains control characters"));
    }
    let url = Url::parse(input.trim()).map_err(|_| ImportError::InvalidUrl("expected a complete HTTPS Figma Design link"))?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("www.figma.com" | "figma.com"))
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ImportError::InvalidUrl("use an HTTPS link on www.figma.com or figma.com"));
    }
    let mut segments = url.path_segments().ok_or(ImportError::InvalidUrl("the link has no file path"))?;
    if !matches!(segments.next(), Some("design" | "file")) {
        return Err(ImportError::InvalidUrl("only Figma Design /design/ and /file/ links are supported"));
    }
    let key = segments.next().filter(|key| valid_key(key)).ok_or(ImportError::InvalidUrl("the file key is missing or invalid"))?;
    // The optional file name is display-only. Additional path segments denote a different
    // resource (including unsupported branch paths), rather than the requested Design file.
    let _name = segments.next();
    if segments.any(|segment| !segment.is_empty()) {
        return Err(ImportError::InvalidUrl("use a Design file link rather than a nested resource link"));
    }
    let node_id = url.query_pairs().find(|(name, _)| name == "node-id").map(|(_, id)| id.into_owned());
    if node_id.as_ref().is_some_and(|id| id.len() > 128 || id.chars().any(char::is_control)) {
        return Err(ImportError::InvalidUrl("the node selection is invalid"));
    }
    Ok(DesignUrl { file_key: key.to_owned(), node_id })
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 128 && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportStage {
    File,
    Variables,
    Images,
    Retry,
}

/// File/Variables report one request, Images report completed image fills, Retry reports seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub stage: ImportStage,
    pub completed: usize,
    pub total: usize,
}

/// Downloaded data for the existing `file.open` and `file.import` commands, without credentials.
#[derive(Clone, Debug)]
pub struct ImportBundle {
    pub file_json: Vec<u8>,
    pub variables_json: Option<Value>,
    pub images: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
    pub file_key: String,
    pub version: Option<String>,
    pub name: String,
}

impl ImportBundle {
    /// An engine command payload. `file.open` creates an unsaved document, so the Figma source
    /// is never overwritten. Missing image bytes are left to the REST importer's placeholders.
    pub fn command_params(&self) -> Value {
        let images: BTreeMap<_, _> = self.images.iter().map(|(reference, bytes)| (reference.clone(), STANDARD.encode(bytes))).collect();
        // The name is a file-name hint: path separators would hide its `.json` extension.
        let hint: String = self.name.chars().map(|c| if matches!(c, '/' | '\\') { '-' } else { c }).collect();
        let mut params = json!({"data": STANDARD.encode(&self.file_json), "name": format!("{hint}.json"), "images": images});
        if let Some(variables) = &self.variables_json {
            params["variables"] = variables.clone();
        }
        params
    }

    /// Small provenance metadata; this is not a live link or a request to resync the document.
    pub fn source(&self) -> Value {
        json!({"fileKey": self.file_key, "version": self.version, "name": self.name})
    }
}

/// Conservative download limits. The total counts every body, including optional JSON and
/// failed image downloads, not just data eventually retained in the bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportLimits {
    pub max_file_bytes: usize,
    pub max_variables_bytes: usize,
    pub max_image_map_bytes: usize,
    pub max_image_bytes: usize,
    pub max_total_bytes: usize,
    pub max_images: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_variables_bytes: 16 * 1024 * 1024,
            max_image_map_bytes: 8 * 1024 * 1024,
            max_image_bytes: 32 * 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            max_images: 2048,
        }
    }
}

/// Messages contain no request URLs, tokens, server response bodies or HTTP-client error text.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ImportError {
    #[error("Invalid Figma link: {0}.")]
    InvalidUrl(&'static str),
    #[error("Enter a valid Figma personal access token (without spaces or control characters).")]
    InvalidToken,
    #[error("Figma import was cancelled.")]
    Cancelled,
    #[error("Could not initialise the HTTPS client.")]
    ClientInit,
    #[error("Could not connect to Figma or download an image. Check your internet connection.")]
    Network,
    #[error("The Figma download timed out. Try again on a faster connection or import a local .fig file.")]
    Timeout,
    #[error("Figma import exceeded its ten-minute time limit.")]
    Deadline,
    #[error("Figma rejected the token (HTTP 401). Check its validity and file_content:read scope.")]
    Unauthorized,
    #[error("Figma denied access (HTTP 403). Check the token, file permissions and file_content:read scope.")]
    Forbidden,
    #[error("Figma could not find the file or image (HTTP 404). Check the link and your access.")]
    NotFound,
    #[error("Figma rate limit reached (HTTP 429; Retry-After seconds: {retry_after_seconds:?}). Retry later.")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("Figma returned HTTP {0}.")]
    HttpStatus(u16),
    #[error("An image URL was refused because it is not a supported HTTPS Figma asset URL.")]
    UnsafeImageUrl,
    #[error("A download redirect was refused.")]
    Redirect,
    #[error("Figma import exceeded the limit for {0}.")]
    Limit(&'static str),
    #[error("Invalid Figma response: {0}.")]
    InvalidResponse(&'static str),
}

#[cfg(test)]
mod tests;
