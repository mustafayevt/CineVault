//! The error shape both transports send.
//!
//! Tauri rejects a command with the serialized error value; the server sends the same object in
//! `{"ok": false, "error": ...}`. One shape keeps `typedError` in the UI working on both.

use serde::{Deserialize, Serialize};
use specta::Type;

/// A command failure the UI can branch on: a stable code plus a sentence for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    /// Stable code, such as `storage`, `bad_filter` or `not_applicable`.
    pub code: String,
    /// One sentence for the user.
    pub message: String,
}

impl CommandError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// A failure that a browser surface has no equivalent for.
    #[must_use]
    pub fn not_applicable(message: impl Into<String>) -> Self {
        Self::new("not_applicable", message)
    }
}

/// The result of a command.
pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_serializes_as_code_and_message() {
        let error = CommandError::not_applicable("The folder is on the server.");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "not_applicable");
        assert_eq!(json["message"], "The folder is on the server.");
        assert_eq!(error.to_string(), "The folder is on the server.");
    }
}
