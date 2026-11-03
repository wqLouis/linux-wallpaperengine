//! A small GLSL parser.
//!
//! The parser is not a complete GLSL front end.  It recognises the shape of
//! a translation unit - global declarations, function definitions and
//! statements - and parses the expressions inside it.  Everything it does
//! not understand becomes an opaque node, which means the normalisation
//! rules simply do not fire there and the source is left alone.
//!
//! The parser records the byte range of every expression it parses.  That is
//! what lets the rewriting be a pure text splice: a rule replaces the range
//! of the expression it applies to and leaves the rest of the file byte for
//! byte identical.

use super::expr::{BinOp, Expr, Span, UnOp};
use super::lexer::{Token, TokenKind, tokenize};
use super::types::{Scalar, Type, parse_type_name};

/// A parsed translation unit.
#[derive(Debug, Default)]
pub struct TranslationUnit {
    /// The global declarations.
    pub globals: Vec<GlobalDecl>,
    /// The function definitions.
    pub functions: Vec<FunctionDef>,
}

/// A global declaration.
#[derive(Debug)]
pub enum GlobalDecl {
    /// A global variable, `TYPE NAME;`.
    Variable {
        /// The type of the variable.
        ty: Type,
        /// The name of the variable.
        name: String,
        /// Whether the variable is an array.
        is_array: bool,
    },
    /// The members of an interface block, such as the uniform block of an
    /// effect.
    InterfaceBlock {
        /// The members of the block.
        members: Vec<Member>,
    },
    /// A declaration the parser does not understand.
    Opaque,
}

/// A named member of an interface block or a parameter of a function.
#[derive(Debug, Clone)]
pub struct Member {
    /// The type of the member.
    pub ty: Type,
    /// The name of the member.
    pub name: String,
    /// Whether the member is an array.
    pub is_array: bool,
}

/// A function definition.
#[derive(Debug)]
pub struct FunctionDef {
    /// The name of the function.
    pub name: String,
    /// The parameters of the function.
    pub params: Vec<Member>,
    /// The body of the function.
    pub body: Block,
}

/// A block of statements.
#[derive(Debug, Default)]
pub struct Block {
    /// The statements of the block.
    pub stmts: Vec<Stmt>,
}

/// A statement.
#[derive(Debug)]
pub enum Stmt {
    /// `TYPE NAME = EXPR;`
    Decl {
        /// The declared type.
        ty: Type,
        /// The name of the variable.
        name: String,
        /// Whether the variable is a constant.
        constant: bool,
        /// The initialiser, if there is one.
        init: Option<Expr>,
    },
    /// `TARGET = VALUE;` or a compound assignment such as `TARGET *= VALUE;`.
    Assign {
        /// The assignment target.
        target: Expr,
        /// The assigned value.
        value: Expr,
        /// The operator of a compound assignment.
        op: Option<BinOp>,
    },
    /// An expression statement.
    Expr(Expr),
    /// `if (COND) BLOCK [else BLOCK]`
    If {
        /// The condition.
        cond: Expr,
        /// The block that is run when the condition holds.
        then_block: Block,
        /// The block that is run otherwise.
        else_block: Option<Block>,
    },
    /// `for (INIT; COND; STEP) BLOCK`
    For {
        /// The initialiser of the loop.
        init: Option<Box<Stmt>>,
        /// The condition of the loop.
        cond: Option<Expr>,
        /// The step of the loop.
        step: Option<Expr>,
        /// The body of the loop.
        body: Block,
    },
    /// `while (COND) BLOCK`
    While {
        /// The condition of the loop.
        cond: Expr,
        /// The body of the loop.
        body: Block,
    },
    /// `do BLOCK while (COND);`
    DoWhile {
        /// The condition of the loop.
        cond: Expr,
        /// The body of the loop.
        body: Block,
    },
    /// `return EXPR;`
    Return(Option<Expr>),
    /// `break;`, `continue;` or `discard;`
    Jump,
    /// A nested block.
    Block(Block),
    /// A statement the parser does not understand.
    Opaque,
}

/// The binding power of the ternary operator.
const BP_TERNARY: u8 = 5;
/// The binding power of `||`.
const BP_LOGICAL_OR: u8 = 10;
/// The binding power of `&&`.
const BP_LOGICAL_AND: u8 = 20;
/// The binding power of `|`.
const BP_BIT_OR: u8 = 30;
/// The binding power of `^`.
const BP_BIT_XOR: u8 = 40;
/// The binding power of `&`.
const BP_BIT_AND: u8 = 50;
/// The binding power of `==` and `!=`.
const BP_EQUALITY: u8 = 60;
/// The binding power of the relational operators.
const BP_RELATIONAL: u8 = 70;
/// The binding power of the shift operators.
const BP_SHIFT: u8 = 80;
/// The binding power of `+` and `-`.
const BP_ADDITIVE: u8 = 90;
/// The binding power of `*`, `/` and `%`.
const BP_MULTIPLICATIVE: u8 = 100;
/// The binding power of the unary operators.
const BP_UNARY: u8 = 120;

/// The kind of an assignment.
#[derive(Debug, Clone, Copy)]
enum AssignOp {
    /// `=`
    Plain,
    /// A compound assignment such as `*=`.
    Compound(BinOp),
}

/// Parse a GLSL source into a translation unit.
pub fn parse(source: &str) -> TranslationUnit {
    let mut parser = Parser::new(source);
    parser.parse_unit()
}

/// The parser.
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    /// A parser for `source`.
    fn new(source: &'a str) -> Self {
        // Comments and preprocessor directives are not part of the grammar
        // the parser knows, so they are dropped here.  The byte ranges of the
        // remaining tokens still refer to the original source.
        let tokens = tokenize(source)
            .into_iter()
            .filter(|token| token.kind != TokenKind::Comment && token.kind != TokenKind::Directive)
            .collect();
        Self {
            source,
            tokens,
            pos: 0,
        }
    }

    /// Whether the parser reached the end of the source.
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// The token the parser is looking at.
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    /// The token `offset` positions after the one the parser is looking at.
    fn peek_at(&self, offset: usize) -> Option<Token> {
        self.tokens.get(self.pos + offset).copied()
    }

    /// The text of the token the parser is looking at.
    fn peek_text(&self) -> &'a str {
        match self.tokens.get(self.pos) {
            Some(token) => token.text(self.source),
            None => "",
        }
    }

    /// Whether the parser is looking at the given punctuation.
    fn at_punct(&self, text: &str) -> bool {
        match self.tokens.get(self.pos) {
            Some(token) => token.kind == TokenKind::Punct && token.text(self.source) == text,
            None => false,
        }
    }

    /// Consume the given punctuation, if it is next.
    fn eat_punct(&mut self, text: &str) -> bool {
        if self.at_punct(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Whether the parser is looking at the given keyword.
    fn at_keyword(&self, text: &str) -> bool {
        match self.tokens.get(self.pos) {
            Some(token) => token.kind == TokenKind::Ident && token.text(self.source) == text,
            None => false,
        }
    }

    /// The byte range of the token that was consumed last.
    fn last_end(&self) -> usize {
        match self.tokens.get(self.pos.saturating_sub(1)) {
            Some(token) => token.end,
            None => 0,
        }
    }

    /// Parse the whole translation unit.
    fn parse_unit(&mut self) -> TranslationUnit {
        let mut unit = TranslationUnit::default();
        loop {
            while self.eat_punct(";") {}
            if self.at_end() {
                break;
            }
            if self.at_punct("}") {
                break;
            }
            let before = self.pos;
            self.parse_global(&mut unit);
            if self.pos == before {
                // Nothing was consumed: skip a token to make progress.
                self.pos += 1;
            }
        }
        unit
    }

    /// Parse a global declaration or a function definition.
    fn parse_global(&mut self, unit: &mut TranslationUnit) {
        let start = self.pos;
        let boundary = self.scan_boundary();
        let header: Vec<Token> = self.tokens[start..boundary].to_vec();
        self.pos = boundary;

        if self.at_punct("{") {
            if let Some(function) = self.parse_function(&header) {
                unit.functions.push(function);
            } else {
                let members = self.parse_block_members();
                if self.is_interface_block(&header) {
                    unit.globals.push(GlobalDecl::InterfaceBlock { members });
                } else {
                    unit.globals.push(GlobalDecl::Opaque);
                }
            }
            self.eat_punct(";");
            return;
        }

        // A global variable, with or without an initialiser.
        if let Some((ty, name, is_array)) = self.split_decl(&header) {
            if self.at_punct("=") {
                self.pos += 1;
                let _ = self.parse_expr(0);
            }
            self.eat_punct(";");
            unit.globals
                .push(GlobalDecl::Variable { ty, name, is_array });
            return;
        }

        // Something the parser does not understand: skip it.
        while !self.at_end() && !self.at_punct(";") {
            self.pos += 1;
        }
        self.eat_punct(";");
        unit.globals.push(GlobalDecl::Opaque);
    }

    /// Parse a function definition, if the header describes one.
    fn parse_function(&mut self, header: &[Token]) -> Option<FunctionDef> {
        let close = header.len().checked_sub(1)?;
        if close == 0 || header[close].kind != TokenKind::Punct {
            return None;
        }
        if header[close].text(self.source) != ")" {
            return None;
        }

        // Find the parenthesis that opens the parameter list.
        let mut depth = 0i32;
        let mut open = None;
        for (index, token) in header.iter().enumerate().rev() {
            let text = token.text(self.source);
            if text == ")" {
                depth += 1;
            } else if text == "(" {
                depth -= 1;
                if depth == 0 {
                    open = Some(index);
                    break;
                }
            }
        }
        let open = open?;
        if open == 0 {
            return None;
        }
        let name_token = header[open - 1];
        if name_token.kind != TokenKind::Ident {
            return None;
        }
        let name = name_token.text(self.source).to_string();
        let params = self.split_params(&header[open + 1..close]);
        let body = self.parse_block();
        Some(FunctionDef { name, params, body })
    }

    /// Whether the header of a block describes an interface block.
    fn is_interface_block(&self, header: &[Token]) -> bool {
        let has_qualifier = header.iter().any(|token| {
            token.kind == TokenKind::Ident
                && matches!(
                    token.text(self.source),
                    "uniform" | "buffer" | "in" | "out" | "inout"
                )
        });
        let has_name = header
            .last()
            .map(|token| token.kind == TokenKind::Ident)
            .unwrap_or(false);
        has_qualifier && has_name
    }

    /// Parse the members of an interface block or a struct.
    fn parse_block_members(&mut self) -> Vec<Member> {
        let mut members = Vec::new();
        if !self.eat_punct("{") {
            return members;
        }
        loop {
            while self.eat_punct(";") {}
            if self.at_end() {
                break;
            }
            if self.at_punct("}") {
                self.pos += 1;
                break;
            }
            let start = self.pos;
            let boundary = self.scan_boundary();
            let header: Vec<Token> = self.tokens[start..boundary].to_vec();
            self.pos = boundary;
            if self.at_punct("=") {
                self.pos += 1;
                let _ = self.parse_expr(0);
            }
            self.eat_punct(";");
            if let Some((ty, name, is_array)) = self.split_decl(&header) {
                members.push(Member { ty, name, is_array });
            }
        }
        members
    }

    /// The index of the first token that ends a declaration.
    ///
    /// The token is a `;`, an `=` or the `{` that opens a block.
    fn scan_boundary(&self) -> usize {
        let mut depth = 0i32;
        let mut index = self.pos;
        while index < self.tokens.len() {
            let token = &self.tokens[index];
            if token.kind != TokenKind::Punct {
                index += 1;
                continue;
            }
            match token.text(self.source) {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = (depth - 1).max(0),
                ";" | "=" | "{" if depth == 0 => return index,
                _ => {}
            }
            index += 1;
        }
        self.tokens.len()
    }

    /// Split a declaration head into its type, its name and whether it is an
    /// array.
    fn split_decl(&self, tokens: &[Token]) -> Option<(Type, String, bool)> {
        let mut index = 0usize;
        while index < tokens.len() {
            let token = tokens[index];
            let text = token.text(self.source);

            // Skip a `layout (...)` qualifier.
            if text == "layout" {
                index += 1;
                if index < tokens.len() && tokens[index].text(self.source) == "(" {
                    let mut depth = 0i32;
                    while index < tokens.len() {
                        let inner = tokens[index].text(self.source);
                        if inner == "(" {
                            depth += 1;
                        } else if inner == ")" {
                            depth -= 1;
                            if depth == 0 {
                                index += 1;
                                break;
                            }
                        }
                        index += 1;
                    }
                }
                continue;
            }

            if token.kind == TokenKind::Ident && is_qualifier(text) {
                index += 1;
                continue;
            }

            if token.kind != TokenKind::Ident {
                return None;
            }
            let ty = parse_type_name(text)?;
            index += 1;
            let name_token = tokens.get(index).copied()?;
            if name_token.kind != TokenKind::Ident {
                return None;
            }
            let name = name_token.text(self.source).to_string();
            index += 1;
            // An array: the type of the elements is not modelled.
            if tokens
                .get(index)
                .map(|token| token.text(self.source) == "[")
                .unwrap_or(false)
            {
                return Some((Type::Unknown, name, true));
            }
            return Some((ty, name, false));
        }
        None
    }

    /// Split a parameter list into its parameters.
    fn split_params(&self, tokens: &[Token]) -> Vec<Member> {
        let mut params = Vec::new();
        let mut current: Vec<Token> = Vec::new();
        let mut depth = 0i32;
        for token in tokens {
            let text = token.text(self.source);
            match text {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = (depth - 1).max(0),
                _ => {}
            }
            if text == "," && depth == 0 {
                if let Some((ty, name, is_array)) = self.split_decl(&current) {
                    params.push(Member { ty, name, is_array });
                }
                current.clear();
                continue;
            }
            current.push(*token);
        }
        if let Some((ty, name, is_array)) = self.split_decl(&current) {
            params.push(Member { ty, name, is_array });
        }
        params
    }

    /// Parse a block of statements.
    fn parse_block(&mut self) -> Block {
        let mut block = Block::default();
        if !self.eat_punct("{") {
            return block;
        }
        loop {
            while self.eat_punct(";") {}
            if self.at_end() {
                break;
            }
            if self.at_punct("}") {
                self.pos += 1;
                break;
            }
            let before = self.pos;
            block.stmts.push(self.parse_stmt());
            if self.pos == before {
                self.pos += 1;
            }
        }
        block
    }

    /// Parse a statement.
    fn parse_stmt(&mut self) -> Stmt {
        if self.at_punct("{") {
            return Stmt::Block(self.parse_block());
        }
        if self.at_keyword("if") {
            return self.parse_if();
        }
        if self.at_keyword("for") {
            return self.parse_for();
        }
        if self.at_keyword("while") {
            return self.parse_while();
        }
        if self.at_keyword("do") {
            return self.parse_do_while();
        }
        if self.at_keyword("switch") {
            return self.parse_switch();
        }
        if self.at_keyword("return") {
            return self.parse_return();
        }
        if self.at_keyword("break") || self.at_keyword("continue") || self.at_keyword("discard") {
            self.pos += 1;
            self.eat_punct(";");
            return Stmt::Jump;
        }
        if self.at_keyword("case") || self.at_keyword("default") {
            while !self.at_end() && !self.at_punct(":") && !self.at_punct("}") {
                self.pos += 1;
            }
            self.eat_punct(":");
            return Stmt::Opaque;
        }
        self.parse_simple_stmt()
    }

    /// Parse a declaration or an expression statement.
    fn parse_simple_stmt(&mut self) -> Stmt {
        let start = self.pos;
        if let Some((ty, name, constant)) = self.parse_decl_head() {
            let init = if self.eat_punct("=") {
                Some(self.parse_expr(0))
            } else {
                None
            };
            self.eat_punct(";");
            return Stmt::Decl {
                ty,
                name,
                constant,
                init,
            };
        }
        self.pos = start;

        let expr = self.parse_expr(0);
        match self.peek_assign_op() {
            Some(AssignOp::Plain) => {
                self.pos += 1;
                let value = self.parse_expr(0);
                self.eat_punct(";");
                Stmt::Assign {
                    target: expr,
                    value,
                    op: None,
                }
            }
            Some(AssignOp::Compound(op)) => {
                self.pos += 1;
                let value = self.parse_expr(0);
                self.eat_punct(";");
                Stmt::Assign {
                    target: expr,
                    value,
                    op: Some(op),
                }
            }
            None => {
                self.eat_punct(";");
                Stmt::Expr(expr)
            }
        }
    }

    /// The assignment operator the parser is looking at.
    fn peek_assign_op(&self) -> Option<AssignOp> {
        match self.peek_text() {
            "=" => Some(AssignOp::Plain),
            "+=" => Some(AssignOp::Compound(BinOp::Add)),
            "-=" => Some(AssignOp::Compound(BinOp::Sub)),
            "*=" => Some(AssignOp::Compound(BinOp::Mul)),
            "/=" => Some(AssignOp::Compound(BinOp::Div)),
            "%=" => Some(AssignOp::Compound(BinOp::Mod)),
            _ => None,
        }
    }

    /// Parse the head of a declaration, if there is one.
    ///
    /// Returns the type, the name and whether the declaration is a
    /// constant.
    fn parse_decl_head(&mut self) -> Option<(Type, String, bool)> {
        let start = self.pos;
        let mut constant = false;
        if self.at_keyword("const") {
            constant = true;
            self.pos += 1;
        }
        let Some(token) = self.peek() else {
            self.pos = start;
            return None;
        };
        if token.kind != TokenKind::Ident {
            self.pos = start;
            return None;
        }
        let Some(ty) = parse_type_name(token.text(self.source)) else {
            self.pos = start;
            return None;
        };
        self.pos += 1;
        let Some(name_token) = self.peek() else {
            self.pos = start;
            return None;
        };
        if name_token.kind != TokenKind::Ident {
            self.pos = start;
            return None;
        }
        let name = name_token.text(self.source).to_string();
        self.pos += 1;
        if self.at_punct("[") {
            // An array: the type of the elements is not modelled.
            while !self.at_end() && !self.at_punct("]") && !self.at_punct(";") {
                self.pos += 1;
            }
            self.eat_punct("]");
            return Some((Type::Unknown, name, constant));
        }
        Some((ty, name, constant))
    }

    /// Parse an `if` statement.
    fn parse_if(&mut self) -> Stmt {
        self.pos += 1;
        let cond = self.parse_paren_expr();
        let then_block = self.parse_body();
        let else_block = if self.at_keyword("else") {
            self.pos += 1;
            Some(self.parse_body())
        } else {
            None
        };
        Stmt::If {
            cond,
            then_block,
            else_block,
        }
    }

    /// Parse a `for` statement.
    fn parse_for(&mut self) -> Stmt {
        self.pos += 1;
        if !self.eat_punct("(") {
            return Stmt::Opaque;
        }
        let init = if self.at_punct(";") {
            None
        } else {
            Some(Box::new(self.parse_simple_stmt()))
        };
        let cond = if self.at_punct(";") {
            None
        } else {
            Some(self.parse_expr(0))
        };
        self.eat_punct(";");
        let step = if self.at_punct(")") {
            None
        } else {
            Some(self.parse_expr(0))
        };
        self.eat_punct(")");
        let body = self.parse_body();
        Stmt::For {
            init,
            cond,
            step,
            body,
        }
    }

    /// Parse a `while` statement.
    fn parse_while(&mut self) -> Stmt {
        self.pos += 1;
        let cond = self.parse_paren_expr();
        let body = self.parse_body();
        Stmt::While { cond, body }
    }

    /// Parse a `do ... while` statement.
    fn parse_do_while(&mut self) -> Stmt {
        self.pos += 1;
        let body = self.parse_body();
        if self.at_keyword("while") {
            self.pos += 1;
        }
        let cond = self.parse_paren_expr();
        self.eat_punct(";");
        Stmt::DoWhile { cond, body }
    }

    /// Parse a `switch` statement.
    fn parse_switch(&mut self) -> Stmt {
        self.pos += 1;
        let _selector = self.parse_paren_expr();
        let body = self.parse_body();
        Stmt::Block(body)
    }

    /// Parse a `return` statement.
    fn parse_return(&mut self) -> Stmt {
        self.pos += 1;
        let value = if self.at_punct(";") {
            None
        } else {
            Some(self.parse_expr(0))
        };
        self.eat_punct(";");
        Stmt::Return(value)
    }

    /// Parse the body of a control statement.
    fn parse_body(&mut self) -> Block {
        if self.at_punct("{") {
            return self.parse_block();
        }
        let mut block = Block::default();
        block.stmts.push(self.parse_stmt());
        block
    }

    /// Parse a parenthesised expression.
    fn parse_paren_expr(&mut self) -> Expr {
        if !self.eat_punct("(") {
            return Expr::Error {
                span: Span::new(0, 0),
            };
        }
        let expr = self.parse_expr(0);
        self.eat_punct(")");
        expr
    }

    /// Parse an expression.
    fn parse_expr(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_unary();
        while let Some((op, left_bp, right_bp)) = self.peek_binary_op() {
            if left_bp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.parse_expr(right_bp);
            let span = Span::new(lhs.span().start, rhs.span().end);
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        if min_bp <= BP_TERNARY && self.at_punct("?") {
            self.pos += 1;
            let then_expr = self.parse_expr(0);
            self.eat_punct(":");
            let other = self.parse_expr(BP_TERNARY);
            let span = Span::new(lhs.span().start, other.span().end);
            lhs = Expr::Ternary {
                cond: Box::new(lhs),
                then_expr: Box::new(then_expr),
                other: Box::new(other),
                span,
            };
        }
        lhs
    }

    /// The binary operator the parser is looking at.
    fn peek_binary_op(&self) -> Option<(BinOp, u8, u8)> {
        let token = self.peek()?;
        if token.kind != TokenKind::Punct {
            return None;
        }
        let (op, bp) = match token.text(self.source) {
            "||" => (BinOp::LogOr, BP_LOGICAL_OR),
            "&&" => (BinOp::LogAnd, BP_LOGICAL_AND),
            "|" => (BinOp::BitOr, BP_BIT_OR),
            "^" => (BinOp::BitXor, BP_BIT_XOR),
            "&" => (BinOp::BitAnd, BP_BIT_AND),
            "==" => (BinOp::Equal, BP_EQUALITY),
            "!=" => (BinOp::NotEqual, BP_EQUALITY),
            "<" => (BinOp::Less, BP_RELATIONAL),
            ">" => (BinOp::Greater, BP_RELATIONAL),
            "<=" => (BinOp::LessEq, BP_RELATIONAL),
            ">=" => (BinOp::GreaterEq, BP_RELATIONAL),
            "<<" => (BinOp::ShiftLeft, BP_SHIFT),
            ">>" => (BinOp::ShiftRight, BP_SHIFT),
            "+" => (BinOp::Add, BP_ADDITIVE),
            "-" => (BinOp::Sub, BP_ADDITIVE),
            "*" => (BinOp::Mul, BP_MULTIPLICATIVE),
            "/" => (BinOp::Div, BP_MULTIPLICATIVE),
            "%" => (BinOp::Mod, BP_MULTIPLICATIVE),
            _ => return None,
        };
        // All the binary operators are left associative.
        Some((op, bp, bp + 1))
    }

    /// Parse a unary expression.
    fn parse_unary(&mut self) -> Expr {
        let Some(token) = self.peek() else {
            return Expr::Error {
                span: Span::new(0, 0),
            };
        };
        if token.kind == TokenKind::Punct {
            let op = match token.text(self.source) {
                "-" => Some(UnOp::Minus),
                "+" => Some(UnOp::Plus),
                "!" => Some(UnOp::Not),
                "~" => Some(UnOp::BitNot),
                "++" | "--" => Some(UnOp::IncDec),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let operand = self.parse_expr(BP_UNARY);
                let span = Span::new(token.start, operand.span().end);
                return Expr::Unary {
                    op,
                    operand: Box::new(operand),
                    span,
                };
            }
        }
        self.parse_postfix()
    }

    /// Parse the postfix operators of an expression.
    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            if self.at_punct(".") {
                let Some(token) = self.peek_at(1) else {
                    break;
                };
                if token.kind != TokenKind::Ident {
                    break;
                }
                self.pos += 2;
                let comps = token.text(self.source).to_string();
                let span = Span::new(expr.span().start, token.end);
                expr = Expr::Swizzle {
                    base: Box::new(expr),
                    comps,
                    span,
                };
                continue;
            }
            if self.at_punct("[") {
                self.pos += 1;
                let index = self.parse_expr(0);
                let mut end = index.span().end;
                if self.eat_punct("]") {
                    end = self.last_end();
                }
                let span = Span::new(expr.span().start, end);
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
                continue;
            }
            if self.at_punct("++") || self.at_punct("--") {
                let Some(token) = self.peek() else {
                    break;
                };
                self.pos += 1;
                let span = Span::new(expr.span().start, token.end);
                expr = Expr::Unary {
                    op: UnOp::IncDec,
                    operand: Box::new(expr),
                    span,
                };
                continue;
            }
            break;
        }
        expr
    }

    /// Parse a primary expression.
    fn parse_primary(&mut self) -> Expr {
        let Some(token) = self.peek() else {
            return Expr::Error {
                span: Span::new(0, 0),
            };
        };

        // A number.
        if token.kind == TokenKind::Number {
            self.pos += 1;
            return Expr::Literal {
                ty: literal_type(token.text(self.source)),
                span: Span::new(token.start, token.end),
            };
        }

        // An identifier: a boolean literal, a call or a variable.
        if token.kind == TokenKind::Ident {
            let text = token.text(self.source);
            if text == "true" || text == "false" {
                self.pos += 1;
                return Expr::Literal {
                    ty: Type::Scalar(Scalar::Bool),
                    span: Span::new(token.start, token.end),
                };
            }
            let is_call = self
                .peek_at(1)
                .map(|next| next.kind == TokenKind::Punct && next.text(self.source) == "(")
                .unwrap_or(false);
            if is_call {
                let name = text.to_string();
                self.pos += 2;
                let mut args = Vec::new();
                if !self.at_punct(")") {
                    loop {
                        args.push(self.parse_expr(0));
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                let mut end = token.end;
                if self.eat_punct(")") {
                    end = self.last_end();
                }
                return Expr::Call {
                    name,
                    args,
                    span: Span::new(token.start, end),
                };
            }
            self.pos += 1;
            return Expr::Ident {
                name: text.to_string(),
                span: Span::new(token.start, token.end),
            };
        }

        // A parenthesised expression.
        if token.kind == TokenKind::Punct && self.at_punct("(") {
            self.pos += 1;
            let inner = self.parse_expr(0);
            let mut end = inner.span().end;
            if self.eat_punct(")") {
                end = self.last_end();
            }
            return Expr::Paren {
                inner: Box::new(inner),
                span: Span::new(token.start, end),
            };
        }

        // Anything else is not something the parser understands.
        self.pos += 1;
        Expr::Error {
            span: Span::new(token.start, token.end),
        }
    }
}

/// Whether the word is a storage or interpolation qualifier.
fn is_qualifier(text: &str) -> bool {
    matches!(
        text,
        "in" | "out"
            | "inout"
            | "uniform"
            | "attribute"
            | "varying"
            | "const"
            | "flat"
            | "smooth"
            | "centroid"
            | "highp"
            | "mediump"
            | "lowp"
            | "precise"
            | "invariant"
            | "coherent"
            | "volatile"
            | "restrict"
            | "readonly"
            | "writeonly"
            | "buffer"
            | "shared"
            | "subroutine"
            | "patch"
            | "sample"
    )
}

/// The type of a numeric literal.
fn literal_type(text: &str) -> Type {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("0x") {
        return if lower.ends_with('u') {
            Type::Scalar(Scalar::Uint)
        } else {
            Type::Scalar(Scalar::Int)
        };
    }
    if lower.ends_with('f') || lower.ends_with("lf") {
        return Type::Scalar(Scalar::Float);
    }
    if lower.contains('.') || lower.contains('e') {
        return Type::Scalar(Scalar::Float);
    }
    if lower.ends_with('u') {
        return Type::Scalar(Scalar::Uint);
    }
    if lower.ends_with('l') {
        return Type::Scalar(Scalar::Int);
    }
    Type::Scalar(Scalar::Int)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_are_recognised() {
        let unit = parse("void main() { float x = 1.0; }");
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "main");
        assert_eq!(unit.functions[0].body.stmts.len(), 1);
    }

    #[test]
    fn uniform_blocks_are_recognised() {
        let source = "layout(binding=1, std140) uniform EffectParams {\n    vec4 g_Foo;\n};\n";
        let unit = parse(source);
        assert_eq!(unit.globals.len(), 1);
        match &unit.globals[0] {
            GlobalDecl::InterfaceBlock { members } => {
                assert_eq!(members.len(), 1);
                assert_eq!(members[0].name, "g_Foo");
                assert_eq!(members[0].ty, Type::Vector(Scalar::Float, 4));
            }
            _ => panic!("expected an interface block"),
        }
    }

    #[test]
    fn attributes_are_recognised() {
        let source = "layout(location=0) in vec4 a_Position;\n";
        let unit = parse(source);
        match &unit.globals[0] {
            GlobalDecl::Variable { ty, name, .. } => {
                assert_eq!(name, "a_Position");
                assert_eq!(*ty, Type::Vector(Scalar::Float, 4));
            }
            _ => panic!("expected a variable"),
        }
    }

    #[test]
    fn binary_expressions_are_left_associative() {
        let unit = parse("void main() { float x = a * b * c; }");
        let Stmt::Decl {
            init: Some(init), ..
        } = &unit.functions[0].body.stmts[0]
        else {
            panic!("expected a declaration");
        };
        let Expr::Binary { lhs, rhs, .. } = init else {
            panic!("expected a binary expression");
        };
        assert!(matches!(lhs.as_ref(), Expr::Binary { .. }));
        assert!(matches!(rhs.as_ref(), Expr::Ident { .. }));
    }

    #[test]
    fn constructor_calls_are_not_declarations() {
        let unit = parse("void main() { vec4(1.0); }");
        assert!(matches!(
            unit.functions[0].body.stmts[0],
            Stmt::Expr(Expr::Call { .. })
        ));
    }
}
