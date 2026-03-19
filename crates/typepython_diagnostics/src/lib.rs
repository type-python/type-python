//! Shared diagnostic primitives for the TypePython workspace.

use std::fmt::{self, Display, Write as _};

use serde::{Deserialize, Serialize};

/// Diagnostic severity aligned with the spec's error reporting model.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A build-blocking error.
    Error,
    /// A non-fatal warning.
    Warning,
    /// An informational note.
    Note,
}

impl Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        })
    }
}

/// Source span attached to a diagnostic.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Path to the source file.
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
    /// 1-based ending line number.
    pub end_line: usize,
    /// 1-based ending column number.
    pub end_column: usize,
}

impl Span {
    /// Creates a new source span.
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self { path: path.into(), line, column, end_line, end_column }
    }
}

/// TypePython diagnostic payload.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable diagnostic code, for example `TPY1001`.
    pub code: String,
    /// Severity of the diagnostic.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Optional supporting notes.
    pub notes: Vec<String>,
    /// Optional source span.
    pub span: Option<Span>,
}

impl Diagnostic {
    /// Creates an error diagnostic.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            notes: Vec::new(),
            span: None,
        }
    }

    /// Creates a warning diagnostic.
    #[must_use]
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            notes: Vec::new(),
            span: None,
        }
    }

    /// Attaches a source span to the diagnostic.
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Appends a note to the diagnostic.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// A collection of diagnostics emitted by one compiler stage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Collected diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticReport {
    /// Pushes a diagnostic into the report.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns `true` if at least one error is present.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// Returns `true` when the report does not contain diagnostics.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Renders the report in a human-readable text format.
    #[must_use]
    pub fn as_text(&self) -> String {
        let mut buffer = String::new();

        for diagnostic in &self.diagnostics {
            let _ = writeln!(
                &mut buffer,
                "{}[{}]: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            );

            if let Some(span) = &diagnostic.span {
                let _ = writeln!(
                    &mut buffer,
                    "  --> {}:{}:{}-{}:{}",
                    span.path, span.line, span.column, span.end_line, span.end_column
                );
            }

            for note in &diagnostic.notes {
                let _ = writeln!(&mut buffer, "  = note: {note}");
            }
        }

        buffer
    }
}

impl Display for DiagnosticReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.as_text())
    }
}
