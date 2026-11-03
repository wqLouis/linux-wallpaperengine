//! The expression AST.
//!
//! Every node carries the byte range of the source it was parsed from, which
//! is what makes the rewriting a pure text splice: a rule replaces the range
//! of the expression it applies to and leaves everything else alone.

use super::types::Type;

/// A byte range in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The first byte.
    pub start: usize,
    /// The byte after the last one.
    pub end: usize,
}

impl Span {
    /// A span covering `start..end`.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The text the span covers.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessEq,
    /// `>=`
    GreaterEq,
    /// `==`
    Equal,
    /// `!=`
    NotEqual,
    /// `&&`
    LogAnd,
    /// `||`
    LogOr,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `<<`
    ShiftLeft,
    /// `>>`
    ShiftRight,
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `!`
    Not,
    /// `~`
    BitNot,
    /// `++` or `--`
    IncDec,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A literal.
    Literal {
        /// The type of the literal.
        ty: Type,
        /// Where the literal is.
        span: Span,
    },
    /// A variable.
    Ident {
        /// The name of the variable.
        name: String,
        /// Where the name is.
        span: Span,
    },
    /// A swizzle, such as `value.xy`.
    Swizzle {
        /// The expression the swizzle is applied to.
        base: Box<Expr>,
        /// The swizzle components.
        comps: String,
        /// Where the swizzle is.
        span: Span,
    },
    /// A function call.
    Call {
        /// The name of the function.
        name: String,
        /// The arguments.
        args: Vec<Expr>,
        /// Where the call is.
        span: Span,
    },
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnOp,
        /// The operand.
        operand: Box<Expr>,
        /// Where the operation is.
        span: Span,
    },
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinOp,
        /// The left hand side.
        lhs: Box<Expr>,
        /// The right hand side.
        rhs: Box<Expr>,
        /// Where the operation is.
        span: Span,
    },
    /// A conditional expression, `cond ? a : b`.
    Ternary {
        /// The condition.
        cond: Box<Expr>,
        /// The value used when the condition holds.
        then_expr: Box<Expr>,
        /// The value used otherwise.
        other: Box<Expr>,
        /// Where the expression is.
        span: Span,
    },
    /// A parenthesised expression.
    Paren {
        /// The expression inside the parentheses.
        inner: Box<Expr>,
        /// Where the parentheses are.
        span: Span,
    },
    /// An array or vector index.
    Index {
        /// The indexed expression.
        base: Box<Expr>,
        /// The index.
        index: Box<Expr>,
        /// Where the index is.
        span: Span,
    },
    /// An expression the parser could not make sense of.
    Error {
        /// Where the offending token is.
        span: Span,
    },
}

impl Expr {
    /// Where the expression is in the source.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. }
            | Expr::Ident { span, .. }
            | Expr::Swizzle { span, .. }
            | Expr::Call { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Ternary { span, .. }
            | Expr::Paren { span, .. }
            | Expr::Index { span, .. }
            | Expr::Error { span } => *span,
        }
    }
}
