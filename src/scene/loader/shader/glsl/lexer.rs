//! A forgiving tokenizer for GLSL source.
//!
//! The tokenizer never fails: it produces the tokens it can recognise and
//! silently skips what it cannot make sense of (string literals, for
//! instance).  Preprocessor directives are kept as a single opaque token so
//! that the parser never has to understand them, and comments are kept as
//! tokens as well so that the source can be reproduced byte for byte.
//!
//! Every token boundary is a character boundary, so the byte ranges the
//! tokenizer produces can be used to slice the source.

/// The kind of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// An identifier or a keyword.
    Ident,
    /// A numeric literal.
    Number,
    /// An operator or a piece of punctuation.
    Punct,
    /// A whole preprocessor directive, kept opaque.
    Directive,
    /// A comment.
    Comment,
}

/// A token, with its byte range in the source.
#[derive(Debug, Clone, Copy)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// The first byte of the token.
    pub start: usize,
    /// The byte after the last one.
    pub end: usize,
}

impl Token {
    /// The text of the token.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// Split `source` into tokens.
///
/// Comments and preprocessor directives are returned as tokens as well; the
/// parser filters them out, but keeping them means the byte ranges of the
/// remaining tokens still cover the original source exactly.
pub fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut at_line_start = true;

    while i < len {
        let byte = bytes[i];

        // Whitespace.
        if byte == b'\n' {
            i += 1;
            at_line_start = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // A preprocessor directive is opaque: it may contain anything.
        if byte == b'#' && at_line_start {
            let start = i;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Directive,
                start,
                end: i,
            });
            continue;
        }
        at_line_start = false;

        // A line comment.
        if byte == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let start = i;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Comment,
                start,
                end: i,
            });
            continue;
        }

        // A block comment.
        if byte == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            tokens.push(Token {
                kind: TokenKind::Comment,
                start,
                end: i,
            });
            continue;
        }

        // A string literal is not valid GLSL, but skipping it keeps the
        // tokenizer from mistaking its contents for code.
        if byte == b'"' {
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                if bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            continue;
        }

        // An identifier.
        if is_ident_start(byte) {
            let start = i;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident,
                start,
                end: i,
            });
            continue;
        }

        // A number.
        if byte.is_ascii_digit() || (byte == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            i = consume_number(bytes, i);
            tokens.push(Token {
                kind: TokenKind::Number,
                start,
                end: i,
            });
            continue;
        }

        // An operator or a piece of punctuation.
        let start = i;
        i = consume_operator(bytes, i);
        tokens.push(Token {
            kind: TokenKind::Punct,
            start,
            end: i,
        });
    }

    tokens
}

/// Whether `byte` may start an identifier.
fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

/// Whether `byte` may appear inside an identifier.
fn is_ident_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// Consume a numeric literal, starting at `start`.
fn consume_number(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    let mut i = start;

    if bytes[i] == b'0' && i + 1 < len && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
        i += 2;
        while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'.') {
            i += 1;
        }
    } else {
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < len && bytes[i] == b'.' {
            i += 1;
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
            let mut exponent = i + 1;
            if exponent < len && (bytes[exponent] == b'+' || bytes[exponent] == b'-') {
                exponent += 1;
            }
            if exponent < len && bytes[exponent].is_ascii_digit() {
                i = exponent;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
    }

    // The `u`, `f` and `l` suffixes.
    while i < len && matches!(bytes[i], b'u' | b'U' | b'f' | b'F' | b'l' | b'L') {
        i += 1;
    }
    i
}

/// The multi-character operators, longest first.
const MULTI_CHAR_OPERATORS: &[&str] = &[
    "<<=", ">>=", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "^^", "+=", "-=",
    "*=", "/=", "%=", "&=", "|=", "^=",
];

/// Consume an operator or a piece of punctuation, starting at `start`.
fn consume_operator(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    for op in MULTI_CHAR_OPERATORS {
        let op_bytes = op.as_bytes();
        if start + op_bytes.len() <= len && &bytes[start..start + op_bytes.len()] == op_bytes {
            return start + op_bytes.len();
        }
    }
    start + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(TokenKind, &str)> {
        tokenize(source)
            .into_iter()
            .map(|t| (t.kind, t.text(source)))
            .collect()
    }

    #[test]
    fn identifiers_numbers_and_operators() {
        let tokens = kinds("vec4 a = 1.0 * 2.0;");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Ident, "vec4"),
                (TokenKind::Ident, "a"),
                (TokenKind::Punct, "="),
                (TokenKind::Number, "1.0"),
                (TokenKind::Punct, "*"),
                (TokenKind::Number, "2.0"),
                (TokenKind::Punct, ";"),
            ]
        );
    }

    #[test]
    fn preprocessor_directives_stay_opaque() {
        let tokens = kinds("#define FOO 1.0\nfloat x = FOO;");
        assert_eq!(tokens[0].0, TokenKind::Directive);
        assert_eq!(tokens[0].1, "#define FOO 1.0");
        // The directive must not swallow the following line.
        assert!(
            tokens
                .iter()
                .any(|(k, t)| *k == TokenKind::Ident && *t == "x")
        );
    }

    #[test]
    fn comments_are_kept_as_tokens() {
        let tokens = kinds("// hello\n/* block */ float x;");
        assert_eq!(tokens[0].0, TokenKind::Comment);
        assert_eq!(tokens[1].0, TokenKind::Comment);
    }

    #[test]
    fn multi_character_operators_are_not_split() {
        let tokens = kinds("a <<= b >> c && d || e == f");
        let puncts: Vec<&str> = tokens
            .iter()
            .filter(|(k, _)| *k == TokenKind::Punct)
            .map(|(_, t)| *t)
            .collect();
        assert_eq!(puncts, ["<<=", ">>", "&&", "||", "=="]);
    }

    #[test]
    fn token_ranges_cover_the_source() {
        let source = "vec4 a = 1.0;\n// comment\nfloat b = 2.0;";
        let tokens = tokenize(source);
        assert!(
            tokens
                .iter()
                .all(|t| t.start <= t.end && t.end <= source.len())
        );
        // Every token range must be a valid character boundary.
        for token in &tokens {
            assert!(source.is_char_boundary(token.start));
            assert!(source.is_char_boundary(token.end));
        }
    }
}
