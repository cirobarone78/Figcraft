//! Native authentication for the local desktop control channel.
use std::ffi::OsString;
use std::fmt;
use std::io;

use figcraft_collab::same_secret;

/// A validated control-channel token. Its contents are intentionally absent from Debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct ControlToken(String);

impl fmt::Debug for ControlToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ControlToken([REDACTED])")
    }
}

impl ControlToken {
    /// Read and validate the process-scoped control token.
    pub fn from_env() -> io::Result<Self> {
        let value = std::env::var_os("FIGCRAFT_CONTROL_TOKEN");
        Self::from_os_value(value)
    }

    /// Validate an explicit Unicode token.
    pub fn new(value: &str) -> io::Result<Self> {
        validate(value)?;
        Ok(Self(value.to_owned()))
    }

    fn from_os_value(value: Option<OsString>) -> io::Result<Self> {
        let value = value
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "FIGCRAFT_CONTROL_TOKEN is required when the control listener is enabled"))?;
        let value = value.into_string().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "FIGCRAFT_CONTROL_TOKEN must be Unicode"))?;
        Self::new(&value)
    }

    /// Compare a presented token with this token using the shared secret comparison helper.
    pub fn matches(&self, presented: &str) -> bool {
        same_secret(&self.0, presented)
    }

    /// Return the value for an authenticated protocol request.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate(value: &str) -> io::Result<()> {
    if !(22..=128).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "FIGCRAFT_CONTROL_TOKEN must be 22 to 128 URL-safe ASCII characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn rejects_missing_empty_and_non_unicode_environment_values() {
        assert!(ControlToken::from_os_value(None).is_err());
        assert!(ControlToken::from_os_value(Some(OsString::new())).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            assert!(ControlToken::from_os_value(Some(OsString::from_vec(vec![0xff]))).is_err());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            assert!(ControlToken::from_os_value(Some(OsString::from_wide(&[0xd800]))).is_err());
        }
    }

    #[test]
    fn accepts_valid_tokens_and_rejects_invalid_tokens() {
        assert!(ControlToken::new("a".repeat(22).as_str()).is_ok());
        assert!(ControlToken::new("a".repeat(128).as_str()).is_ok());
        let invalid = [String::new(), "a".repeat(21), "a".repeat(129), "contains space 1234567890".into(), "unicode-é-123456789012".into()];
        for value in invalid {
            assert!(ControlToken::new(&value).is_err());
        }
        let token = ControlToken::new("a_secure_token_with_32_chars").unwrap();
        assert!(token.matches("a_secure_token_with_32_chars"));
        assert!(!token.matches("another_secure_token_with_32"));
        assert!(!format!("{token:?}").contains("secure"));
    }
}
