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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionApplicability {
    MachineApplicable,
    MaybeIncorrect,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSuggestion {
    pub message: String,
    pub span: Span,
    pub replacement: String,
    pub applicability: SuggestionApplicability,
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
    /// Optional machine-readable fix suggestions.
    pub suggestions: Vec<DiagnosticSuggestion>,
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
            suggestions: Vec::new(),
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
            suggestions: Vec::new(),
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

    /// Appends a machine-readable suggestion to the diagnostic.
    #[must_use]
    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
        applicability: SuggestionApplicability,
    ) -> Self {
        self.suggestions.push(DiagnosticSuggestion {
            message: message.into(),
            span,
            replacement: replacement.into(),
            applicability,
        });
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
            for suggestion in &diagnostic.suggestions {
                let _ = writeln!(
                    &mut buffer,
                    "  = help: {} (replace {}:{}:{}-{}:{} with `{}`; applicability: {:?})",
                    suggestion.message,
                    suggestion.span.path,
                    suggestion.span.line,
                    suggestion.span.column,
                    suggestion.span.end_line,
                    suggestion.span.end_column,
                    suggestion.replacement,
                    suggestion.applicability,
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_display_renders_lowercase() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Note.to_string(), "note");
    }

    #[test]
    fn span_new_stores_all_fields() {
        let span = Span::new("src/main.py", 10, 5, 10, 20);
        assert_eq!(span.path, "src/main.py");
        assert_eq!(span.line, 10);
        assert_eq!(span.column, 5);
        assert_eq!(span.end_line, 10);
        assert_eq!(span.end_column, 20);
    }

    #[test]
    fn error_diagnostic_has_error_severity() {
        let diag = Diagnostic::error("TPY1001", "type mismatch");
        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.code, "TPY1001");
        assert_eq!(diag.message, "type mismatch");
        assert!(diag.notes.is_empty());
        assert!(diag.suggestions.is_empty());
        assert!(diag.span.is_none());
    }

    #[test]
    fn warning_diagnostic_has_warning_severity() {
        let diag = Diagnostic::warning("TPY2001", "unused variable");
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.code, "TPY2001");
        assert_eq!(diag.message, "unused variable");
    }

    #[test]
    fn diagnostic_with_span_attaches_span() {
        let span = Span::new("lib.py", 3, 1, 3, 10);
        let diag = Diagnostic::error("TPY1002", "undefined name").with_span(span.clone());
        assert_eq!(diag.span, Some(span));
    }

    #[test]
    fn diagnostic_with_note_appends_note() {
        let diag = Diagnostic::error("TPY1003", "bad call").with_note("expected 2 arguments");
        assert_eq!(diag.notes, vec!["expected 2 arguments"]);
    }

    #[test]
    fn diagnostic_with_multiple_notes() {
        let diag = Diagnostic::warning("TPY2002", "ambiguous type")
            .with_note("first note")
            .with_note("second note");
        assert_eq!(diag.notes.len(), 2);
        assert_eq!(diag.notes[0], "first note");
        assert_eq!(diag.notes[1], "second note");
    }

    #[test]
    fn diagnostic_with_suggestion_attaches_suggestion() {
        let span = Span::new("app.py", 5, 1, 5, 4);
        let diag = Diagnostic::error("TPY1004", "wrong type").with_suggestion(
            "use str instead",
            span.clone(),
            "str",
            SuggestionApplicability::MachineApplicable,
        );
        assert_eq!(diag.suggestions.len(), 1);
        let s = &diag.suggestions[0];
        assert_eq!(s.message, "use str instead");
        assert_eq!(s.span, span);
        assert_eq!(s.replacement, "str");
        assert_eq!(s.applicability, SuggestionApplicability::MachineApplicable);
    }

    #[test]
    fn report_is_empty_for_default_report() {
        let report = DiagnosticReport::default();
        assert!(report.is_empty());
        assert!(!report.has_errors());
    }

    #[test]
    fn report_push_adds_diagnostic() {
        let mut report = DiagnosticReport::default();
        report.push(Diagnostic::error("TPY1001", "err"));
        assert_eq!(report.diagnostics.len(), 1);
        assert!(!report.is_empty());
    }

    #[test]
    fn report_has_errors_returns_true_for_error() {
        let mut report = DiagnosticReport::default();
        report.push(Diagnostic::warning("TPY2001", "warn"));
        report.push(Diagnostic::error("TPY1001", "err"));
        assert!(report.has_errors());
    }

    #[test]
    fn report_has_errors_returns_false_for_warnings_only() {
        let mut report = DiagnosticReport::default();
        report.push(Diagnostic::warning("TPY2001", "warn1"));
        report.push(Diagnostic::warning("TPY2002", "warn2"));
        assert!(!report.has_errors());
    }

    #[test]
    fn report_as_text_formats_error_with_span_notes_and_suggestions() {
        let mut report = DiagnosticReport::default();
        report.push(
            Diagnostic::error("TPY1001", "type mismatch")
                .with_span(Span::new("main.py", 10, 5, 10, 20))
                .with_note("expected int, got str")
                .with_suggestion(
                    "cast to int",
                    Span::new("main.py", 10, 5, 10, 20),
                    "int(x)",
                    SuggestionApplicability::MaybeIncorrect,
                ),
        );
        let text = report.as_text();
        assert!(text.contains("error[TPY1001]: type mismatch"));
        assert!(text.contains("  --> main.py:10:5-10:20"));
        assert!(text.contains("  = note: expected int, got str"));
        assert!(text.contains("  = help: cast to int (replace main.py:10:5-10:20 with `int(x)`; applicability: MaybeIncorrect)"));
    }

    #[test]
    fn report_display_matches_as_text() {
        let mut report = DiagnosticReport::default();
        report.push(Diagnostic::warning("TPY2001", "unused import"));
        assert_eq!(format!("{report}"), report.as_text());
    }

    #[test]
    fn diagnostic_serde_round_trip() {
        let diag = Diagnostic::error("TPY1005", "missing return")
            .with_span(Span::new("foo.py", 1, 1, 2, 1))
            .with_note("function declared with return type")
            .with_suggestion(
                "add return statement",
                Span::new("foo.py", 2, 1, 2, 1),
                "return None",
                SuggestionApplicability::MachineApplicable,
            );
        let json = serde_json::to_string(&diag).expect("serialize");
        let deserialized: Diagnostic = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(diag, deserialized);
    }

    #[test]
    fn report_serde_round_trip() {
        let mut report = DiagnosticReport::default();
        report.push(Diagnostic::error("TPY1001", "err").with_note("n1"));
        report.push(Diagnostic::warning("TPY2001", "warn"));
        let json = serde_json::to_string(&report).expect("serialize");
        let deserialized: DiagnosticReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report.diagnostics, deserialized.diagnostics);
    }
}
