//! Kroa lexer with significant indentation (spaces only; tabs are rejected).

use crate::diagnostics::{DiagnosticCode, Diagnostics};
use crate::span::{SourceFile, Span};
use crate::token::{Token, TokenKind};

pub fn lex(file: &SourceFile, diagnostics: &mut Diagnostics) -> Option<Vec<Token>> {
    let mut lexer = Lexer {
        source: &file.source,
        pos: 0,
        diagnostics,
        tokens: Vec::new(),
        indent_stack: vec![0],
        at_line_start: true,
        pending_dedents: 0,
    };
    lexer.lex_all();
    if lexer.diagnostics.has_errors() {
        None
    } else {
        Some(lexer.tokens)
    }
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    diagnostics: &'a mut Diagnostics,
    tokens: Vec<Token>,
    indent_stack: Vec<usize>,
    at_line_start: bool,
    pending_dedents: usize,
}

impl<'a> Lexer<'a> {
    fn lex_all(&mut self) {
        while self.pending_dedents > 0 {
            self.push(TokenKind::Dedent, self.pos, self.pos);
            self.pending_dedents -= 1;
        }

        while !self.is_eof() {
            if self.at_line_start && !self.handle_indentation() {
                continue;
            }

            while self.pending_dedents > 0 {
                self.push(TokenKind::Dedent, self.pos, self.pos);
                self.pending_dedents -= 1;
            }

            if self.is_eof() {
                break;
            }

            let ch = self.peek_char();
            match ch {
                ' ' => {
                    // Spaces inside a line are insignificant.
                    self.bump_char();
                }
                '\t' => {
                    let start = self.pos;
                    self.bump_char();
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(start, self.pos),
                            DiagnosticCode::E0100,
                            "tabs are not allowed; indent with spaces only",
                        )
                        .help("configure your editor to insert spaces, then re-indent this line"),
                    );
                }
                '\r' => {
                    self.bump_char();
                    if self.peek_char() == '\n' {
                        self.bump_char();
                    }
                    self.push(TokenKind::Newline, self.pos.saturating_sub(1), self.pos);
                    self.at_line_start = true;
                }
                '\n' => {
                    let start = self.pos;
                    self.bump_char();
                    self.push(TokenKind::Newline, start, self.pos);
                    self.at_line_start = true;
                }
                '#' => self.skip_comment(),
                '0'..='9' => self.lex_number(),
                'a'..='z' | 'A'..='Z' | '_' => self.lex_ident_or_keyword(),
                '"' => self.lex_string(),
                _ => self.lex_symbol(),
            }
        }

        // Emit remaining dedents at EOF.
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.push(TokenKind::Dedent, self.pos, self.pos);
        }
        self.push(TokenKind::Eof, self.pos, self.pos);
    }

    /// Returns false when the whole line was blank/comment and should be skipped.
    fn handle_indentation(&mut self) -> bool {
        let start = self.pos;
        let mut spaces = 0usize;

        loop {
            match self.peek_char() {
                ' ' => {
                    spaces += 1;
                    self.bump_char();
                }
                '\t' => {
                    let tab_start = self.pos;
                    self.bump_char();
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(tab_start, self.pos),
                            DiagnosticCode::E0100,
                            "tabs are not allowed; indent with spaces only",
                        )
                        .help("configure your editor to insert spaces, then re-indent this line"),
                    );
                }
                '#' => {
                    self.skip_comment();
                    // Comment-only line: consume newline if present, stay at line start.
                    if self.peek_char() == '\r' {
                        self.bump_char();
                    }
                    if self.peek_char() == '\n' {
                        self.bump_char();
                    }
                    self.at_line_start = true;
                    return false;
                }
                '\r' => {
                    self.bump_char();
                    if self.peek_char() == '\n' {
                        self.bump_char();
                    }
                    self.at_line_start = true;
                    return false;
                }
                '\n' => {
                    self.bump_char();
                    self.at_line_start = true;
                    return false;
                }
                '\0' => {
                    self.at_line_start = false;
                    return true;
                }
                _ => break,
            }
        }

        self.at_line_start = false;
        let current = *self.indent_stack.last().unwrap();
        if spaces == current {
            return true;
        }
        if spaces > current {
            self.indent_stack.push(spaces);
            self.push(TokenKind::Indent, start, self.pos);
            return true;
        }

        // Dedent to a previous level.
        while let Some(&top) = self.indent_stack.last() {
            if top == spaces {
                break;
            }
            if top < spaces {
                self.diagnostics.push(
                    crate::diagnostics::Diagnostic::error_at_code(
                        Span::new(start, self.pos),
                        DiagnosticCode::E0101,
                        format!(
                            "inconsistent indentation: found {spaces} spaces, expected one of {:?}",
                            self.indent_stack
                        ),
                    )
                    .help(
                        "use a consistent number of spaces for each nesting level (for example 4)",
                    ),
                );
                break;
            }
            self.indent_stack.pop();
            self.pending_dedents += 1;
        }
        true
    }

    fn skip_comment(&mut self) {
        while !self.is_eof() {
            let ch = self.peek_char();
            if ch == '\n' || ch == '\r' {
                break;
            }
            self.bump_char();
        }
    }

    fn lex_number(&mut self) {
        let start = self.pos;
        while self.peek_char().is_ascii_digit() {
            self.bump_char();
        }
        if self.peek_char() == '.' && self.peek_char_at(1).is_ascii_digit() {
            self.bump_char();
            while self.peek_char().is_ascii_digit() {
                self.bump_char();
            }
            let text = &self.source[start..self.pos];
            match text.parse::<f64>() {
                Ok(v) => self.push(TokenKind::Float(v), start, self.pos),
                Err(_) => self.diagnostics.error_at(
                    Span::new(start, self.pos),
                    format!("invalid float literal `{text}`"),
                ),
            }
        } else {
            let text = &self.source[start..self.pos];
            match text.parse::<i64>() {
                Ok(v) => self.push(TokenKind::Int(v), start, self.pos),
                Err(_) => self.diagnostics.error_at(
                    Span::new(start, self.pos),
                    format!("invalid integer literal `{text}`"),
                ),
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) {
        let start = self.pos;
        self.bump_char();
        while matches!(
            self.peek_char(),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_'
        ) {
            self.bump_char();
        }
        let text = &self.source[start..self.pos];
        let kind = TokenKind::keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        self.push(kind, start, self.pos);
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        self.bump_char(); // opening "
        let mut value = String::new();
        while !self.is_eof() {
            match self.peek_char() {
                '"' => {
                    self.bump_char();
                    self.push(TokenKind::StringLit(value), start, self.pos);
                    return;
                }
                '\\' => {
                    self.bump_char();
                    let esc = self.peek_char();
                    let mapped = match esc {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '"' => '"',
                        '0' => '\0',
                        _ => {
                            let esc_start = self.pos;
                            self.bump_char();
                            self.diagnostics.error_at(
                                Span::new(esc_start, self.pos),
                                format!("unknown escape sequence `\\{esc}`"),
                            );
                            continue;
                        }
                    };
                    self.bump_char();
                    value.push(mapped);
                }
                '\n' | '\r' => {
                    self.diagnostics
                        .error_at(Span::new(start, self.pos), "unterminated string literal");
                    return;
                }
                ch => {
                    value.push(ch);
                    self.bump_char();
                }
            }
        }
        self.diagnostics
            .error_at(Span::new(start, self.pos), "unterminated string literal");
    }

    fn lex_symbol(&mut self) {
        let start = self.pos;
        let ch = self.bump_char();
        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            ';' => TokenKind::Semi,
            '+' => TokenKind::Plus,
            '%' => TokenKind::Percent,
            ':' => {
                if self.peek_char() == ':' {
                    self.bump_char();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            '-' => {
                if self.peek_char() == '>' {
                    self.bump_char();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '=' => {
                if self.peek_char() == '=' {
                    self.bump_char();
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek_char() == '=' {
                    self.bump_char();
                    TokenKind::NotEq
                } else {
                    // Canonical form is `not` (not `!`) — keep grammar unambiguous for agents.
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(start, self.pos),
                            DiagnosticCode::E0201,
                            "unexpected `!`; Kroa uses the keyword `not` for logical negation",
                        )
                        .help("replace `!expr` with `not expr`"),
                    );
                    return;
                }
            }
            '<' => {
                if self.peek_char() == '=' {
                    self.bump_char();
                    TokenKind::LtEq
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek_char() == '=' {
                    self.bump_char();
                    TokenKind::GtEq
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.peek_char() == '&' {
                    let amp_start = start;
                    self.bump_char();
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(amp_start, self.pos),
                            DiagnosticCode::E0201,
                            "unexpected `&&`; Kroa uses the keyword `and` for logical conjunction",
                        )
                        .help("replace `&&` with `and`"),
                    );
                    return;
                } else {
                    TokenKind::Amp
                }
            }
            '|' => {
                if self.peek_char() == '|' {
                    let pipe_start = start;
                    self.bump_char();
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(pipe_start, self.pos),
                            DiagnosticCode::E0201,
                            "unexpected `||`; Kroa uses the keyword `or` for logical disjunction",
                        )
                        .help("replace `||` with `or`"),
                    );
                    return;
                } else {
                    self.diagnostics.push(
                        crate::diagnostics::Diagnostic::error_at_code(
                            Span::new(start, self.pos),
                            DiagnosticCode::E0201,
                            "unexpected character `|`; use `or` for logical disjunction",
                        )
                        .help("replace `|` / `||` with `or`"),
                    );
                    return;
                }
            }
            _ => {
                self.diagnostics.error_at(
                    Span::new(start, self.pos),
                    format!("unexpected character `{ch}`"),
                );
                return;
            }
        };
        self.push(kind, start, self.pos);
    }

    fn push(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, end),
        });
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn peek_char(&self) -> char {
        self.source[self.pos..].chars().next().unwrap_or('\0')
    }

    fn peek_char_at(&self, n: usize) -> char {
        self.source[self.pos..].chars().nth(n).unwrap_or('\0')
    }

    fn bump_char(&mut self) -> char {
        let ch = self.peek_char();
        if ch != '\0' {
            self.pos += ch.len_utf8();
        }
        ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostics;
    use crate::span::SourceFile;

    fn lex_ok(src: &str) -> Vec<TokenKind> {
        let file = SourceFile::new("test.kroa".into(), src.into());
        let mut d = Diagnostics::new();
        let tokens = lex(&file, &mut d).expect("lex failed");
        assert!(!d.has_errors(), "{}", d.render_all(&file));
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn rejects_tabs() {
        let file = SourceFile::new("t.kroa".into(), "\tlet x = 1\n".into());
        let mut d = Diagnostics::new();
        assert!(lex(&file, &mut d).is_none());
        assert!(d.has_errors());
    }

    #[test]
    fn emits_indent_dedent() {
        let kinds = lex_ok("fn main() -> i64:\n    return 1\n");
        assert!(kinds.contains(&TokenKind::Indent));
        assert!(kinds.contains(&TokenKind::Dedent));
        assert!(kinds.contains(&TokenKind::Fn));
    }
}
