//! Token definitions for the Kroa lexer.

use crate::span::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    While,
    Return,
    True,
    False,
    Extern,
    Struct,
    Arena,
    Unsafe,
    As,
    And,
    Or,
    Not,

    // Identifiers and literals
    Ident(String),
    Int(i64),
    Float(f64),
    StringLit(String),

    // Types / punctuation used as names are just Idents; operators below
    Arrow, // ->
    Colon, // :
    Comma, // ,
    Dot,   // .
    Semi,  // ;

    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    LBracket, // [
    RBracket, // ]

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,    // =
    EqEq,  // ==
    NotEq, // !=
    Lt,
    LtEq,
    Gt,
    GtEq,
    Amp,        // &
    ColonColon, // ::

    // Indentation
    Newline,
    Indent,
    Dedent,

    Eof,
}

impl TokenKind {
    pub fn keyword(name: &str) -> Option<TokenKind> {
        Some(match name {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "extern" => TokenKind::Extern,
            "struct" => TokenKind::Struct,
            "arena" => TokenKind::Arena,
            "unsafe" => TokenKind::Unsafe,
            "as" => TokenKind::As,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            _ => return None,
        })
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TokenKind::Fn => "fn",
            TokenKind::Let => "let",
            TokenKind::Mut => "mut",
            TokenKind::If => "if",
            TokenKind::Else => "else",
            TokenKind::While => "while",
            TokenKind::Return => "return",
            TokenKind::True => "true",
            TokenKind::False => "false",
            TokenKind::Extern => "extern",
            TokenKind::Struct => "struct",
            TokenKind::Arena => "arena",
            TokenKind::Unsafe => "unsafe",
            TokenKind::As => "as",
            TokenKind::And => "and",
            TokenKind::Or => "or",
            TokenKind::Not => "not",
            TokenKind::Ident(s) => return write!(f, "identifier `{s}`"),
            TokenKind::Int(n) => return write!(f, "integer {n}"),
            TokenKind::Float(n) => return write!(f, "float {n}"),
            TokenKind::StringLit(_) => "string literal",
            TokenKind::Arrow => "->",
            TokenKind::Colon => ":",
            TokenKind::Comma => ",",
            TokenKind::Dot => ".",
            TokenKind::Semi => ";",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LBracket => "[",
            TokenKind::RBracket => "]",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::Eq => "=",
            TokenKind::EqEq => "==",
            TokenKind::NotEq => "!=",
            TokenKind::Lt => "<",
            TokenKind::LtEq => "<=",
            TokenKind::Gt => ">",
            TokenKind::GtEq => ">=",
            TokenKind::Amp => "&",
            TokenKind::ColonColon => "::",
            TokenKind::Newline => "newline",
            TokenKind::Indent => "indent",
            TokenKind::Dedent => "dedent",
            TokenKind::Eof => "end of file",
        };
        write!(f, "{s}")
    }
}
