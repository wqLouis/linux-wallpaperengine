//! The C expression evaluator of the `#if` conditions.
//!
//! The condition of a preprocessor directive is a constant expression of the
//! C language, and the Wallpaper Engine shaders use the whole set of the
//! operators:
//!
//! ```text
//! #if SKINNING
//! #if LIGHTING || REFLECTION
//! #if (LIGHTING || REFLECTION) && EMISSIVE_MAP
//! #if TEX4FORMAT == FORMAT_R8 || TEX4FORMAT == FORMAT_RG88
//! #if TEX1FORMAT >= FORMAT_ETC1_RGB8 && TEX1FORMAT <= FORMAT_DXT1 || TEX1FORMAT == FORMAT_BC7
//! ```
//!
//! A textual evaluation of these conditions gets the precedence wrong, so
//! this module parses them: the condition is tokenised, the `defined`
//! operator is resolved, the macros are expanded and the result is computed
//! by a recursive descent parser that implements the precedence and the
//! associativity of C.
//!
//! The evaluator is deliberately lenient where C is not: a macro whose value
//! is not an integer (a float, for example) is zero, and a division by zero
//! is zero instead of an error.  A condition that cannot be parsed at all is
//! an error, and the caller decides what to do with it.

use crate::collections::{Map, new_map};

/// A macro definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macro {
    /// The replacement list of the macro: the text it expands to.
    pub body: String,
    /// The parameters of a function-like macro.
    pub params: Vec<String>,
    /// Whether the macro is function-like (`#define NAME(a, b) ...`).
    pub function_like: bool,
}

impl Macro {
    /// A macro that expands to a fixed text.
    pub fn object(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            params: Vec::new(),
            function_like: false,
        }
    }
}

/// The macros that are visible while a condition is evaluated.
pub type MacroMap = Map<String, Macro>;

/// Build a macro table from a plain map of macro values.
///
/// Every macro of the table is object-like: a function-like macro loses its
/// parameter list, which only matters for a macro that is used in a
/// condition - something the Wallpaper Engine shaders never do.
pub fn macro_map_from_values(values: &Map<String, String>) -> MacroMap {
    let mut map = new_map();
    for (name, value) in values {
        map.insert(name.clone(), Macro::object(value.clone()));
    }
    map
}

/// A token of a preprocessor condition.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// An identifier.
    Ident(String),
    /// An integer literal.
    Int(i64),
    /// A floating point literal.  A condition may not contain one, but a
    /// macro may, and such a macro is then not expanded.
    Float,
    /// An operator or a punctuation mark.
    Punct(&'static str),
}

/// The operators of a condition.
///
/// The two character operators come first, so that `<<` is not read as `<`
/// followed by `<`.
const OPERATORS: &[&str] = &[
    "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "&", "|", "^", "~",
    "!", "<", ">", "(", ")", ",",
];

/// The maximum depth of the macro expansion.
///
/// A macro that expands to itself would expand forever, so the expansion is
/// bounded.  A macro that is being expanded is not expanded again (the
/// "blue paint" rule of the C standard), but two macros that expand to each
/// other still need the bound.
const MAX_EXPANSION_DEPTH: usize = 32;

/// Evaluate a preprocessor condition.
///
/// `defines` holds the macros that are visible.  The result is `Ok(true)`
/// when the condition holds and `Ok(false)` when it does not.  A condition
/// that cannot be parsed is an error.
pub fn evaluate(condition: &str, defines: &MacroMap) -> Result<bool, String> {
    let tokens = tokenize(condition)?;
    let tokens = resolve_defined(tokens, defines)?;
    let mut expander = Expander {
        defines,
        active: Vec::new(),
        depth: 0,
    };
    let tokens = expander.expand(tokens);
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.parse_expression()?;
    Ok(value != 0)
}

/// Split a condition into its tokens.
fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        // An identifier.
        if byte == b'_' || byte.is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len()
                && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            tokens.push(Token::Ident(input[start..index].to_string()));
            continue;
        }

        // A number.
        if byte.is_ascii_digit() {
            let (token, next) = lex_number(input, index)?;
            tokens.push(token);
            index = next;
            continue;
        }

        // A character literal.
        if byte == b'\'' {
            let (token, next) = lex_char(input, index)?;
            tokens.push(token);
            index = next;
            continue;
        }

        // An operator.
        let rest = &input[index..];
        let mut matched = false;
        for op in OPERATORS {
            if rest.starts_with(op) {
                tokens.push(Token::Punct(op));
                index += op.len();
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(format!("the character `{}` is not allowed", byte as char));
        }
    }

    Ok(tokens)
}

/// Read a numeric literal.
///
/// Decimal, hexadecimal (`0x`), octal and binary (`0b`) literals are
/// supported, with an optional `u`, `U`, `l` or `L` suffix.
fn lex_number(input: &str, start: usize) -> Result<(Token, usize), String> {
    let bytes = input.as_bytes();
    let radix =
        if bytes[start] == b'0' && start + 1 < bytes.len() && matches!(bytes[start + 1] | 32, b'x')
        {
            16
        } else if bytes[start] == b'0'
            && start + 1 < bytes.len()
            && matches!(bytes[start + 1] | 32, b'b')
        {
            2
        } else {
            10
        };
    let mut index = start + if radix == 10 { 0 } else { 2 };

    let digits_start = index;
    let is_digit = |byte: u8| match radix {
        16 => byte.is_ascii_hexdigit(),
        2 => matches!(byte, b'0' | b'1'),
        _ => byte.is_ascii_digit(),
    };
    while index < bytes.len() && (is_digit(bytes[index]) || bytes[index] == b'_') {
        index += 1;
    }

    let digits: String = input[digits_start..index]
        .chars()
        .filter(|c| *c != '_')
        .collect();
    if digits.is_empty() {
        return Err("a numeric literal without digits".to_string());
    }

    // A floating point literal.
    let mut is_float = false;
    if radix == 10 {
        if index < bytes.len() && bytes[index] == b'.' {
            is_float = true;
            index += 1;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
                index += 1;
            }
        }
        if index < bytes.len() && matches!(bytes[index] | 32, b'e') {
            is_float = true;
            index += 1;
            if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
                index += 1;
            }
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
                index += 1;
            }
        }
    }

    // The suffix.
    while index < bytes.len() && matches!(bytes[index] | 32, b'u' | b'l') {
        index += 1;
    }

    if is_float {
        return Ok((Token::Float, index));
    }

    // A leading zero means an octal literal.
    let value = if radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(&digits[1..], 8)
    } else {
        i64::from_str_radix(&digits, radix)
    }
    .map_err(|_| format!("the literal `{}` does not fit into an integer", digits))?;

    Ok((Token::Int(value), index))
}

/// Read a character literal.
fn lex_char(input: &str, start: usize) -> Result<(Token, usize), String> {
    let bytes = input.as_bytes();
    let mut index = start + 1;
    if index >= bytes.len() {
        return Err("an unterminated character literal".to_string());
    }

    let value = if bytes[index] == b'\\' {
        index += 1;
        if index >= bytes.len() {
            return Err("an unterminated character literal".to_string());
        }
        let escaped = bytes[index];
        index += 1;
        match escaped {
            b'n' => 10,
            b't' => 9,
            b'r' => 13,
            b'0' => 0,
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'v' => 11,
            b'\\' => 92,
            b'\'' => 39,
            b'"' => 34,
            other => other as i64,
        }
    } else {
        let value = bytes[index] as i64;
        index += 1;
        value
    };

    if index >= bytes.len() || bytes[index] != b'\'' {
        return Err("an unterminated character literal".to_string());
    }

    Ok((Token::Int(value), index + 1))
}

/// Replace the `defined` operator by its value.
///
/// The operator is resolved before the macros are expanded, and its operand
/// is never expanded: `defined(A)` asks whether the macro `A` exists, not
/// what it expands to.
fn resolve_defined(tokens: Vec<Token>, defines: &MacroMap) -> Result<Vec<Token>, String> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut index = 0;

    while index < tokens.len() {
        let is_defined = matches!(&tokens[index], Token::Ident(name) if name == "defined");
        if !is_defined {
            result.push(tokens[index].clone());
            index += 1;
            continue;
        }

        let mut next = index + 1;
        let parenthesised = matches!(tokens.get(next), Some(Token::Punct("(")));
        if parenthesised {
            next += 1;
        }

        let Some(Token::Ident(name)) = tokens.get(next).cloned() else {
            return Err("`defined` without a macro name".to_string());
        };
        next += 1;

        if parenthesised {
            if tokens.get(next) != Some(&Token::Punct(")")) {
                return Err("`defined(` without a `)`".to_string());
            }
            next += 1;
        }

        result.push(Token::Int(i64::from(defines.contains_key(&name))));
        index = next;
    }

    Ok(result)
}

/// The macro expander of a condition.
struct Expander<'a> {
    /// The macros that can be expanded.
    defines: &'a MacroMap,
    /// The macros that are currently being expanded.  A macro that is being
    /// expanded is not expanded again.
    active: Vec<String>,
    /// The current depth of the expansion.
    depth: usize,
}

impl Expander<'_> {
    /// Expand every macro of the token list.
    fn expand(&mut self, tokens: Vec<Token>) -> Vec<Token> {
        if self.depth >= MAX_EXPANSION_DEPTH {
            return tokens;
        }
        self.depth += 1;

        let mut result = Vec::with_capacity(tokens.len());
        let mut index = 0;
        while index < tokens.len() {
            let Token::Ident(name) = &tokens[index] else {
                result.push(tokens[index].clone());
                index += 1;
                continue;
            };

            match self.expand_macro(name, &tokens, &mut index) {
                Some(expanded) => result.extend(expanded),
                None => {
                    result.push(tokens[index].clone());
                    index += 1;
                }
            }
        }

        self.depth -= 1;
        result
    }

    /// Expand a single macro.
    ///
    /// Returns `None` when the macro is not expanded at all: it is not
    /// defined, it is being expanded already, it is a function-like macro
    /// that is not called, or its value is not an integer expression.
    fn expand_macro(
        &mut self,
        name: &str,
        tokens: &[Token],
        index: &mut usize,
    ) -> Option<Vec<Token>> {
        if self.active.iter().any(|active| active == name) {
            return None;
        }
        let definition = self.defines.get(name)?.clone();

        if definition.function_like {
            // A function-like macro is only expanded when it is called.
            if tokens.get(*index + 1) != Some(&Token::Punct("(")) {
                return None;
            }
            let (arguments, end) = split_arguments(tokens, *index + 1)?;
            if arguments.len() != definition.params.len() {
                return None;
            }
            let body = tokenize(&definition.body).ok()?;
            if body.iter().any(|token| matches!(token, Token::Float)) {
                return None;
            }
            let substituted = substitute(&body, &definition.params, &arguments);
            *index = end;
            self.active.push(name.to_string());
            let expanded = self.expand(substituted);
            self.active.pop();
            Some(expanded)
        } else {
            let body = tokenize(&definition.body).ok()?;
            if body.iter().any(|token| matches!(token, Token::Float)) {
                return None;
            }
            *index += 1;
            self.active.push(name.to_string());
            let expanded = self.expand(body);
            self.active.pop();
            Some(expanded)
        }
    }
}

/// Split the argument list of a function-like macro.
///
/// `open` is the index of the opening parenthesis.  The result is the
/// arguments and the index behind the closing parenthesis.
fn split_arguments(tokens: &[Token], open: usize) -> Option<(Vec<Vec<Token>>, usize)> {
    if tokens.get(open) != Some(&Token::Punct("(")) {
        return None;
    }

    let mut arguments: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut depth = 0usize;
    let mut index = open + 1;

    while index < tokens.len() {
        match &tokens[index] {
            Token::Punct("(") => {
                depth += 1;
                current.push(tokens[index].clone());
            }
            Token::Punct(")") => {
                if depth == 0 {
                    if !current.is_empty() || !arguments.is_empty() {
                        arguments.push(current);
                    }
                    return Some((arguments, index + 1));
                }
                depth -= 1;
                current.push(tokens[index].clone());
            }
            Token::Punct(",") if depth == 0 => {
                arguments.push(std::mem::take(&mut current));
            }
            other => current.push(other.clone()),
        }
        index += 1;
    }

    None
}

/// Replace the parameters of a function-like macro by its arguments.
fn substitute(body: &[Token], params: &[String], arguments: &[Vec<Token>]) -> Vec<Token> {
    let mut result = Vec::with_capacity(body.len());
    for token in body {
        let Token::Ident(name) = token else {
            result.push(token.clone());
            continue;
        };
        match params.iter().position(|param| param == name) {
            Some(index) => match arguments.get(index) {
                Some(argument) => result.extend(argument.iter().cloned()),
                None => result.push(token.clone()),
            },
            None => result.push(token.clone()),
        }
    }
    result
}

/// The parser of a condition.
struct Parser {
    /// The tokens of the condition.
    tokens: Vec<Token>,
    /// The index of the next token.
    pos: usize,
}

impl Parser {
    /// Parse the whole condition.
    ///
    /// Nothing may follow the expression: a token that is left over is an
    /// error.
    fn parse_expression(&mut self) -> Result<i64, String> {
        let value = self.parse_ternary()?;
        if let Some(token) = self.tokens.get(self.pos) {
            return Err(format!("`{}` is unexpected", describe(token)));
        }
        Ok(value)
    }

    /// Parse a conditional expression: `condition ? then : other`.
    fn parse_ternary(&mut self) -> Result<i64, String> {
        let condition = self.parse_binary(1)?;
        if self.peek_op() != Some("?") {
            return Ok(condition);
        }
        self.pos += 1;
        let then_value = self.parse_ternary()?;
        if self.peek_op() != Some(":") {
            return Err("a `?` without a `:`".to_string());
        }
        self.pos += 1;
        let other_value = self.parse_ternary()?;
        Ok(if condition != 0 {
            then_value
        } else {
            other_value
        })
    }

    /// Parse a binary expression.
    ///
    /// `min_precedence` is the precedence of the operators that are not part
    /// of the expression any more: the operators of a higher precedence bind
    /// tighter, the operators of the same precedence are left associative.
    fn parse_binary(&mut self, min_precedence: u8) -> Result<i64, String> {
        let mut lhs = self.parse_unary()?;

        while let Some(op) = self.peek_op() {
            let Some(precedence) = precedence(op) else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            self.pos += 1;
            let rhs = self.parse_binary(precedence + 1)?;
            lhs = apply_binary(op, lhs, rhs);
        }

        Ok(lhs)
    }

    /// Parse a unary expression.
    fn parse_unary(&mut self) -> Result<i64, String> {
        let Some(op) = self.peek_op() else {
            return self.parse_primary();
        };
        if !matches!(op, "!" | "~" | "-" | "+") {
            return self.parse_primary();
        }

        self.pos += 1;
        let value = self.parse_unary()?;
        Ok(match op {
            "!" => i64::from(value == 0),
            "~" => !value,
            "-" => value.wrapping_neg(),
            _ => value,
        })
    }

    /// Parse a primary expression: a literal, an identifier or a parenthesised
    /// expression.
    fn parse_primary(&mut self) -> Result<i64, String> {
        match self.tokens.get(self.pos).cloned() {
            Some(Token::Int(value)) => {
                self.pos += 1;
                Ok(value)
            }
            Some(Token::Ident(_)) => {
                // An identifier that is still here after the expansion is a
                // macro that is not defined: it is zero.
                self.pos += 1;
                Ok(0)
            }
            Some(Token::Punct("(")) => {
                self.pos += 1;
                let value = self.parse_ternary()?;
                if self.peek_op() != Some(")") {
                    return Err("a `(` without a `)`".to_string());
                }
                self.pos += 1;
                Ok(value)
            }
            Some(Token::Float) => {
                Err("a floating point literal is not allowed in a condition".to_string())
            }
            Some(token) => Err(format!("`{}` is unexpected", describe(&token))),
            None => Err("the condition ends unexpectedly".to_string()),
        }
    }

    /// The operator of the next token, if the next token is one.
    fn peek_op(&self) -> Option<&'static str> {
        match self.tokens.get(self.pos) {
            Some(Token::Punct(op)) => Some(op),
            _ => None,
        }
    }
}

/// The precedence of a binary operator.
///
/// The higher the number, the tighter the operator binds.
fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

/// Apply a binary operator to its operands.
fn apply_binary(op: &str, lhs: i64, rhs: i64) -> i64 {
    match op {
        "||" => i64::from(lhs != 0 || rhs != 0),
        "&&" => i64::from(lhs != 0 && rhs != 0),
        "|" => lhs | rhs,
        "^" => lhs ^ rhs,
        "&" => lhs & rhs,
        "==" => i64::from(lhs == rhs),
        "!=" => i64::from(lhs != rhs),
        "<" => i64::from(lhs < rhs),
        ">" => i64::from(lhs > rhs),
        "<=" => i64::from(lhs <= rhs),
        ">=" => i64::from(lhs >= rhs),
        "<<" => lhs.wrapping_shl((rhs as u32) & 63),
        ">>" => lhs.wrapping_shr((rhs as u32) & 63),
        "+" => lhs.wrapping_add(rhs),
        "-" => lhs.wrapping_sub(rhs),
        "*" => lhs.wrapping_mul(rhs),
        // A division by zero is zero: the C standard leaves it undefined, but
        // a shader that divides by zero must not take the application down.
        "/" => {
            if rhs == 0 {
                0
            } else {
                lhs.wrapping_div(rhs)
            }
        }
        "%" => {
            if rhs == 0 {
                0
            } else {
                lhs.wrapping_rem(rhs)
            }
        }
        _ => 0,
    }
}

/// A description of a token, for the error messages.
fn describe(token: &Token) -> String {
    match token {
        Token::Ident(name) => name.clone(),
        Token::Int(value) => value.to_string(),
        Token::Float => "a floating point literal".to_string(),
        Token::Punct(op) => (*op).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evaluate a condition with a set of macros.
    fn eval(condition: &str, defines: &[(&str, &str)]) -> bool {
        let mut map = new_map();
        for (name, value) in defines {
            map.insert(name.to_string(), Macro::object(value.to_string()));
        }
        evaluate(condition, &map).unwrap_or_else(|error| {
            panic!("the condition `{condition}` could not be evaluated: {error}");
        })
    }

    /// A condition of the Wallpaper Engine shaders: the condition, the macros
    /// it is evaluated with and the result the C preprocessor computes.
    type ConditionCase = (&'static str, &'static [(&'static str, &'static str)], bool);

    /// The conditions of the Wallpaper Engine shaders, with the result the C
    /// preprocessor computes.
    const CASES: &[ConditionCase] = &[
        ("A == 1 && B == 2", &[("A", "1"), ("B", "2")], true),
        ("A == 1 || B == 2", &[("A", "1"), ("B", "2")], true),
        ("(A || B) && C", &[("A", "1"), ("B", "1"), ("C", "1")], true),
        ("A && (B || C)", &[("A", "1"), ("B", "0"), ("C", "1")], true),
        ("A >= 1", &[("A", "2")], true),
        ("defined(A) && defined(B)", &[("A", "1"), ("B", "1")], true),
        ("defined(A) || defined(B)", &[("A", "1")], true),
        (
            "TEX4FORMAT == FORMAT_R8 || TEX4FORMAT == FORMAT_RG88",
            &[
                ("TEX4FORMAT", "9"),
                ("FORMAT_R8", "9"),
                ("FORMAT_RG88", "8"),
            ],
            true,
        ),
        (
            "HDR && (LIGHTING || REFLECTION) && EMISSIVE_MAP",
            &[
                ("HDR", "1"),
                ("LIGHTING", "1"),
                ("REFLECTION", "0"),
                ("EMISSIVE_MAP", "1"),
            ],
            true,
        ),
        ("NORMALMAP == 0", &[("NORMALMAP", "0")], true),
        ("COMPOSITE != 0", &[("COMPOSITE", "3")], true),
    ];

    #[test]
    fn the_conditions_of_the_shaders_are_evaluated_like_c_does() {
        for (condition, defines, expected) in CASES {
            assert_eq!(
                eval(condition, defines),
                *expected,
                "the condition `{condition}` is evaluated wrongly"
            );
        }
    }

    #[test]
    fn the_operators_have_the_precedence_of_c() {
        // The comparison binds tighter than the conjunction.
        assert!(eval("1 + 1 == 2 && 3 * 2 == 6", &[]));
        // The conjunction binds tighter than the disjunction: the condition
        // is `1 || (1 && 0)` and not `(1 || 1) && 0`.
        assert!(eval("0 && 0 || 1", &[]));
        assert!(eval("1 || 1 && 0", &[]));
        // The shift binds tighter than the comparison.
        assert!(eval("1 << 2 == 4", &[]));
        // The equality binds looser than the relational operator.
        assert!(eval("1 < 2 == 1", &[]));
        // The unary operators bind tighter than the binary ones.
        assert!(eval("!0 && 1", &[]));
        assert!(eval("-1 < 0", &[]));
        assert!(eval("~0 == -1", &[]));
    }

    #[test]
    fn the_parentheses_change_the_result() {
        assert!(!eval("(1 || 0) && 0", &[]));
        assert!(eval("1 || (0 && 0)", &[]));
    }

    #[test]
    fn a_macro_is_expanded_before_it_is_evaluated() {
        assert!(eval(
            "TEX1FORMAT == FORMAT_R8",
            &[("TEX1FORMAT", "9"), ("FORMAT_R8", "9")]
        ));
        // A macro whose value is an expression.
        assert!(eval("TWO == 2", &[("TWO", "(1 + 1)")]));
        // A macro that is used twice.
        assert!(eval("A == A", &[("A", "3")]));
    }

    #[test]
    fn a_macro_that_expands_to_itself_is_not_expanded_forever() {
        assert!(!eval("SELF == 1", &[("SELF", "SELF")]));
    }

    #[test]
    fn a_function_like_macro_is_expanded_when_it_is_called() {
        let mut map = new_map();
        map.insert(
            "TWICE".to_string(),
            Macro {
                body: "((x) * 2)".to_string(),
                params: vec!["x".to_string()],
                function_like: true,
            },
        );
        assert!(evaluate("TWICE(3) == 6", &map).unwrap());
        // A function-like macro that is not called is not expanded.
        assert!(!evaluate("TWICE == 6", &map).unwrap());
    }

    #[test]
    fn the_literals_are_read_the_way_c_reads_them() {
        assert!(eval("0x10 == 16", &[]));
        assert!(eval("010 == 8", &[]));
        assert!(eval("0b101 == 5", &[]));
        assert!(eval("10u == 10", &[]));
        assert!(eval("'A' == 65", &[]));
    }

    #[test]
    fn a_division_by_zero_is_zero() {
        assert!(!eval("1 / 0", &[]));
        assert!(!eval("1 % 0", &[]));
    }

    #[test]
    fn an_undefined_macro_is_zero() {
        assert!(!eval("UNDEFINED", &[]));
        assert!(eval("UNDEFINED == 0", &[]));
        assert!(!eval("defined(UNDEFINED)", &[]));
    }

    #[test]
    fn the_defined_operator_has_both_spellings() {
        assert!(eval("defined(A)", &[("A", "1")]));
        assert!(eval("defined A", &[("A", "1")]));
        assert!(!eval("!defined(A)", &[("A", "1")]));
        assert!(!eval("!defined A", &[("A", "1")]));
    }

    #[test]
    fn a_condition_that_cannot_be_parsed_is_an_error() {
        let map = new_map();
        assert!(evaluate("1 +", &map).is_err());
        assert!(evaluate("(1", &map).is_err());
        assert!(evaluate("1 1", &map).is_err());
    }
}
