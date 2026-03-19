//! Language-server boundary for TypePython.

use thiserror::Error;

/// LSP startup error.
#[derive(Debug, Error)]
pub enum LspError {
    /// Placeholder state while the real LSP server is not implemented.
    #[error("the TypePython LSP server is scaffolded but not implemented yet")]
    NotImplemented,
}

/// Starts the placeholder LSP service.
pub fn serve() -> Result<(), LspError> {
    Err(LspError::NotImplemented)
}
