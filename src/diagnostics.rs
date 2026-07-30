//! Compiler diagnostics with agent-friendly structured output.
//!
//! Human format (stderr):
//! ```text
//! error[E0301]: message
//!   --> file.kroa:3:5
//!    |
//!  3 |     ...
//!    |     ^^^
//!   note: ...
//!   help: ...
//! ```
//!
//! JSON format (`--message-format json`): one JSON object per diagnostic, NDJSON.

use crate::span::{SourceFile, Span};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Stable diagnostic codes for agents and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    /// Generic / uncategorized error
    E0000,
    /// Tab character rejected
    E0100,
    /// Inconsistent indentation
    E0101,
    /// Unexpected token / parse error
    E0200,
    /// Redundant / forbidden syntax form (grammar must stay unambiguous)
    E0201,
    /// Undefined name
    E0300,
    /// Type mismatch
    E0301,
    /// Immutable assignment
    E0302,
    /// Use after move
    E0303,
    /// Return type mismatch
    E0304,
    /// Borrow conflict
    E0400,
    /// Assign while borrowed
    E0401,
    /// Move while borrowed
    E0402,
    /// Reference escapes arena / local scope
    E0403,
    /// Arena enter/exit mismatch
    E0404,
    /// FFI / unsafe boundary
    E0500,
    /// I/O or driver error
    E0900,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::E0000 => "E0000",
            DiagnosticCode::E0100 => "E0100",
            DiagnosticCode::E0101 => "E0101",
            DiagnosticCode::E0200 => "E0200",
            DiagnosticCode::E0201 => "E0201",
            DiagnosticCode::E0300 => "E0300",
            DiagnosticCode::E0301 => "E0301",
            DiagnosticCode::E0302 => "E0302",
            DiagnosticCode::E0303 => "E0303",
            DiagnosticCode::E0304 => "E0304",
            DiagnosticCode::E0400 => "E0400",
            DiagnosticCode::E0401 => "E0401",
            DiagnosticCode::E0402 => "E0402",
            DiagnosticCode::E0403 => "E0403",
            DiagnosticCode::E0404 => "E0404",
            DiagnosticCode::E0500 => "E0500",
            DiagnosticCode::E0900 => "E0900",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub span: Option<Span>,
    /// Resolved source path (filled by `attach_location`).
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, DiagnosticCode::E0000, message)
    }

    pub fn error_code(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn error_at(span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, DiagnosticCode::E0000, message).with_span(span)
    }

    pub fn error_at_code(span: Span, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message).with_span(span)
    }

    pub fn warning_at(span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, DiagnosticCode::E0000, message).with_span(span)
    }

    fn new(severity: Severity, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            span: None,
            file: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Fill file/line/column fields from a source file and span.
    pub fn attach_location(&mut self, file: &SourceFile) {
        self.file = Some(file.name.clone());
        if let Some(span) = self.span {
            let (line, col) = file.line_col(span.start);
            let (end_line, end_col) = file.line_col(span.end);
            self.line = Some(line);
            self.column = Some(col);
            self.end_line = Some(end_line);
            self.end_column = Some(end_col.max(col));
        }
    }

    pub fn render(&self, file: Option<&SourceFile>) -> String {
        let mut out = String::new();
        let kind = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let code = self.code.as_str();

        let file_name = self
            .file
            .as_deref()
            .or_else(|| file.map(|f| f.name.as_str()));
        let line = self
            .line
            .or_else(|| file.and_then(|f| self.span.map(|s| f.line_col(s.start).0)));
        let col = self
            .column
            .or_else(|| file.and_then(|f| self.span.map(|s| f.line_col(s.start).1)));

        match (file_name, line, col) {
            (Some(name), Some(line), Some(col)) => {
                out.push_str(&format!(
                    "{kind}[{code}]: {name}:{line}:{col}: {}\n",
                    self.message
                ));
                let src = file;
                if let Some(src) = src {
                    let text = src.line_text(line);
                    out.push_str("  |\n");
                    out.push_str(&format!("{line:>3} | {text}\n"));
                    out.push_str("  | ");
                    let prefix_cols = col.saturating_sub(1);
                    out.push_str(&" ".repeat(prefix_cols));
                    let len = self
                        .span
                        .map(|s| s.end.saturating_sub(s.start).max(1))
                        .unwrap_or(1);
                    out.push_str(&"^".repeat(len.min(40)));
                    out.push('\n');
                } else {
                    out.push_str(&format!("  --> {name}:{line}:{col}\n"));
                }
            }
            _ => {
                out.push_str(&format!("{kind}[{code}]: {}\n", self.message));
            }
        }

        for note in &self.notes {
            out.push_str(&format!("  note: {note}\n"));
        }
        if let Some(help) = &self.help {
            out.push_str(&format!("  help: {help}\n"));
        }
        out
    }

    /// One-line NDJSON object suitable for agent auto-fix loops.
    pub fn to_json_line(&self) -> String {
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let mut s = String::from("{");
        push_json_str(&mut s, "severity", severity);
        s.push(',');
        push_json_str(&mut s, "code", self.code.as_str());
        s.push(',');
        push_json_str(&mut s, "message", &self.message);
        if let Some(file) = &self.file {
            s.push(',');
            push_json_str(&mut s, "file", file);
        }
        if let Some(line) = self.line {
            s.push(',');
            s.push_str(&format!("\"line\":{line}"));
        }
        if let Some(column) = self.column {
            s.push(',');
            s.push_str(&format!("\"column\":{column}"));
        }
        if let Some(end_line) = self.end_line {
            s.push(',');
            s.push_str(&format!("\"end_line\":{end_line}"));
        }
        if let Some(end_column) = self.end_column {
            s.push(',');
            s.push_str(&format!("\"end_column\":{end_column}"));
        }
        if !self.notes.is_empty() {
            s.push(',');
            s.push_str("\"notes\":[");
            for (i, note) in self.notes.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                push_json_raw_string(&mut s, note);
            }
            s.push(']');
        }
        if let Some(help) = &self.help {
            s.push(',');
            push_json_str(&mut s, "help", help);
        }
        s.push('}');
        s
    }
}

fn push_json_str(out: &mut String, key: &str, value: &str) {
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    push_json_raw_string(out, value);
}

fn push_json_raw_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render(None))
    }
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(Diagnostic::error(message));
    }

    pub fn error_code(&mut self, code: DiagnosticCode, message: impl Into<String>) {
        self.push(Diagnostic::error_code(code, message));
    }

    pub fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.push(Diagnostic::error_at(span, message));
    }

    pub fn error_at_code(&mut self, span: Span, code: DiagnosticCode, message: impl Into<String>) {
        self.push(Diagnostic::error_at_code(span, code, message));
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Resolve file/line/column on every diagnostic that has a span.
    pub fn attach_locations(&mut self, file: &SourceFile) {
        for d in &mut self.items {
            d.attach_location(file);
        }
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn render_all(&self, file: &SourceFile) -> String {
        let mut out = String::new();
        for d in &self.items {
            out.push_str(&d.render(Some(file)));
            out.push('\n');
        }
        out
    }

    pub fn render_json_all(&self) -> String {
        let mut out = String::new();
        for d in &self.items {
            out.push_str(&d.to_json_line());
            out.push('\n');
        }
        out
    }
}

/// How the CLI should print diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageFormat {
    #[default]
    Human,
    Json,
}

impl MessageFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "human" => Some(MessageFormat::Human),
            "json" => Some(MessageFormat::Json),
            _ => None,
        }
    }
}

pub fn print_diagnostics(diags: &[Diagnostic], format: MessageFormat) {
    match format {
        MessageFormat::Human => {
            for d in diags {
                eprint!("{d}");
            }
        }
        MessageFormat::Json => {
            for d in diags {
                eprintln!("{}", d.to_json_line());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::SourceFile;

    #[test]
    fn json_contains_coordinates() {
        let file = SourceFile::new(
            "demo.kroa".into(),
            "fn main() -> i64:\n    return true\n".into(),
        );
        let mut d = Diagnostic::error_at_code(
            Span::new(20, 24),
            DiagnosticCode::E0304,
            "return type mismatch: expected `i64`, found `bool`",
        )
        .help("change the returned value to `i64`, or change the function return type");
        d.attach_location(&file);
        let json = d.to_json_line();
        assert!(json.contains("\"code\":\"E0304\""), "{json}");
        assert!(json.contains("\"file\":\"demo.kroa\""), "{json}");
        assert!(json.contains("\"line\":"), "{json}");
        assert!(json.contains("\"column\":"), "{json}");
        assert!(json.contains("\"help\":"), "{json}");
    }
}
