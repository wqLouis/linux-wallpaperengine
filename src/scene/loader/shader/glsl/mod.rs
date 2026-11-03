//! A type-aware normalisation pipeline for GLSL sources.
//!
//! Wallpaper Engine's own shader compiler is lenient in a way strict GLSL
//! is not: where GLSL rejects an operation because two operands have a
//! different number of components, the reference engine truncates the
//! longer vector to the component count of the shorter one.  A shader that
//! relies on that leniency compiles with Wallpaper Engine but is rejected
//! by naga, the shader compiler wgpu uses:
//!
//! ```text
//! naga: "Operation Multiply can't work with Vector{Quad} and Vector{Bi}"
//! ```
//!
//! This module reproduces the leniency generically.  The pipeline is:
//!
//! 1. [`lexer`] tokenises the source.  Preprocessor directives stay opaque
//!    and comments are preserved.
//! 2. [`parser`] parses the translation unit.  Every expression carries the
//!    byte range it was parsed from.
//! 3. [`symbols`] holds the symbol table: the uniforms of the effect (from
//!    the layout that was collected when the scene was loaded), the
//!    attributes and varyings of the shader, the locals and constants of a
//!    function and the built-in globals of GLSL.
//! 4. [`infer`] works out the type of an expression.  Anything it cannot
//!    make sense of is [`types::Type::Unknown`].
//! 5. [`fixups`] holds the normalisation rules.  They are table driven, so
//!    new rules can be added without touching the parser.
//!
//! The rewriting is a text splice: only the ranges of the expressions that
//! actually violate a rule are replaced, the rest of the file stays byte for
//! byte identical.

pub mod expr;
pub mod fixups;
pub mod infer;
pub mod lexer;
pub mod parser;
pub mod symbols;
pub mod types;

use self::expr::{Expr, Span};
use self::fixups::{AssignArgs, BinaryArgs, Rewritten, Side, SwizzleInfo};
use self::infer::{binary_result_type, call_result_type, swizzle_type};
use self::parser::{Block, GlobalDecl, Stmt, TranslationUnit};
use self::symbols::{SymbolKind, SymbolTable};
use self::types::{Scalar, Type};
use super::layout::EffectLayout;

/// A replacement of a range of the source.
#[derive(Debug, Clone)]
struct Edit {
    /// The range that is replaced.
    span: Span,
    /// The text that replaces it.
    text: String,
}

/// Normalise `source` so that it satisfies the strict typing rules of GLSL.
///
/// The types of the uniforms and varyings of the shader are taken from
/// `layout`; everything else is worked out from the source itself.
pub fn normalise(source: &str, layout: &EffectLayout) -> String {
    let unit = parser::parse(source);
    let mut rewriter = Rewriter::new(source, layout, &unit);
    rewriter.run(&unit);
    apply_edits(source, rewriter.edits)
}

/// Replace the ranges of the source the edits cover.
fn apply_edits(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| edit.span.start);
    let mut result = String::with_capacity(source.len());
    let mut position = 0usize;
    for edit in edits {
        if edit.span.start < position {
            // Overlapping edits cannot happen, but skipping one is safer
            // than producing a broken shader.
            continue;
        }
        result.push_str(&source[position..edit.span.start]);
        result.push_str(&edit.text);
        position = edit.span.end;
    }
    result.push_str(&source[position..]);
    result
}

/// The rewriter walks the parsed translation unit and collects the edits the
/// normalisation rules ask for.
struct Rewriter<'a> {
    source: &'a str,
    symbols: SymbolTable,
    edits: Vec<Edit>,
    /// The function that is currently rewritten, for the log messages.
    function: String,
}

impl<'a> Rewriter<'a> {
    /// A rewriter for `source`.
    fn new(source: &'a str, layout: &EffectLayout, unit: &TranslationUnit) -> Self {
        let mut rewriter = Self {
            source,
            symbols: SymbolTable::new(),
            edits: Vec::new(),
            function: String::new(),
        };
        rewriter.seed_globals(unit, layout);
        rewriter
    }

    /// Seed the symbol table with everything the shader can refer to.
    ///
    /// The layout comes first: it knows the uniforms of the effect and the
    /// varyings of the shader interface, including the ones the shader does
    /// not declare itself.  The declarations of the shader come second:
    /// they are the ones that survive the preprocessing, so they describe
    /// the shader that is actually compiled.
    fn seed_globals(&mut self, unit: &TranslationUnit, layout: &EffectLayout) {
        for (name, declared) in &layout.uniform_decls {
            self.symbols.declare_global(
                name,
                types::parse_declared_type(declared),
                SymbolKind::Uniform,
                false,
            );
        }
        for (name, declared) in &layout.varying_types {
            let is_array = layout.varying_array_sizes.get(name).copied().unwrap_or(1) > 1;
            self.symbols.declare_global(
                name,
                types::parse_declared_type(declared),
                SymbolKind::Varying,
                is_array,
            );
        }
        for global in &unit.globals {
            match global {
                GlobalDecl::Variable { ty, name, is_array } => {
                    self.symbols
                        .declare_global(name, *ty, SymbolKind::Attribute, *is_array);
                }
                GlobalDecl::InterfaceBlock { members } => {
                    for member in members {
                        self.symbols.declare_global(
                            &member.name,
                            member.ty,
                            SymbolKind::Uniform,
                            member.is_array,
                        );
                    }
                }
                GlobalDecl::Opaque => {}
            }
        }
    }

    /// Rewrite the whole translation unit.
    fn run(&mut self, unit: &TranslationUnit) {
        for function in &unit.functions {
            self.function = function.name.clone();
            self.symbols.push_scope();
            for param in &function.params {
                self.symbols
                    .declare(&param.name, param.ty, SymbolKind::Parameter);
            }
            self.rewrite_block(&function.body);
            self.symbols.pop_scope();
        }
        self.function.clear();
    }

    /// Rewrite a block of statements.
    fn rewrite_block(&mut self, block: &Block) {
        self.symbols.push_scope();
        for stmt in &block.stmts {
            self.rewrite_stmt(stmt);
        }
        self.symbols.pop_scope();
    }

    /// Rewrite a statement.
    fn rewrite_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Decl {
                ty,
                name,
                constant,
                init,
            } => {
                // The variable is in scope in its own initialiser.
                let kind = if *constant {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Local
                };
                self.symbols.declare(name, *ty, kind);
                if let Some(expr) = init {
                    let rewritten = self.rewrite_expr(expr);
                    let rewritten = self.apply_assignment_rules(*ty, rewritten);
                    self.push_edit(expr.span(), rewritten.text);
                }
            }
            Stmt::Assign { target, value, op } => {
                let rewritten = self.rewrite_expr(value);
                let target_type = infer::infer(target, &self.symbols);
                let rewritten = match op {
                    None => self.apply_assignment_rules(target_type, rewritten),
                    Some(_) => self.apply_compound_rules(target_type, rewritten),
                };
                self.push_edit(value.span(), rewritten.text);
            }
            Stmt::Expr(expr) => {
                let rewritten = self.rewrite_expr(expr);
                self.push_edit(expr.span(), rewritten.text);
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
            } => {
                let rewritten = self.rewrite_expr(cond);
                self.push_edit(cond.span(), rewritten.text);
                self.rewrite_block(then_block);
                if let Some(block) = else_block {
                    self.rewrite_block(block);
                }
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                self.symbols.push_scope();
                if let Some(init) = init {
                    self.rewrite_stmt(init);
                }
                if let Some(cond) = cond {
                    let rewritten = self.rewrite_expr(cond);
                    self.push_edit(cond.span(), rewritten.text);
                }
                if let Some(step) = step {
                    let rewritten = self.rewrite_expr(step);
                    self.push_edit(step.span(), rewritten.text);
                }
                self.rewrite_block(body);
                self.symbols.pop_scope();
            }
            Stmt::While { cond, body } => {
                let rewritten = self.rewrite_expr(cond);
                self.push_edit(cond.span(), rewritten.text);
                self.rewrite_block(body);
            }
            Stmt::DoWhile { cond, body } => {
                let rewritten = self.rewrite_expr(cond);
                self.push_edit(cond.span(), rewritten.text);
                self.rewrite_block(body);
            }
            Stmt::Return(value) => {
                if let Some(expr) = value {
                    let rewritten = self.rewrite_expr(expr);
                    self.push_edit(expr.span(), rewritten.text);
                }
            }
            Stmt::Jump | Stmt::Opaque => {}
            Stmt::Block(block) => self.rewrite_block(block),
        }
    }

    /// Rewrite an expression, returning its new text and its type.
    ///
    /// The type that is returned is the type the expression has *after* the
    /// rules were applied, so that the rules of the enclosing expression
    /// see the corrected types.
    fn rewrite_expr(&mut self, expr: &Expr) -> Rewritten {
        match expr {
            Expr::Literal { ty, .. } => Rewritten {
                text: self.text_of(expr),
                ty: *ty,
                needs_parens: false,
                swizzle: None,
            },
            Expr::Ident { name, .. } => Rewritten {
                text: self.text_of(expr),
                ty: self.symbols.lookup_type(name).unwrap_or(Type::Unknown),
                needs_parens: false,
                swizzle: None,
            },
            Expr::Swizzle { base, comps, .. } => {
                let rewritten = self.rewrite_expr(base);
                let text = self.rebuild(expr.span(), &[(base.span(), rewritten.text.clone())]);
                Rewritten {
                    ty: swizzle_type(rewritten.ty, comps),
                    text,
                    needs_parens: false,
                    swizzle: Some(SwizzleInfo {
                        base: rewritten.text,
                        comps: comps.clone(),
                    }),
                }
            }
            Expr::Call { name, args, .. } => {
                let mut children = Vec::with_capacity(args.len());
                let mut arg_types = Vec::with_capacity(args.len());
                for arg in args {
                    let rewritten = self.rewrite_expr(arg);
                    arg_types.push(rewritten.ty);
                    children.push((arg.span(), rewritten.text));
                }
                let text = self.rebuild(expr.span(), &children);
                Rewritten {
                    ty: call_result_type(name, &arg_types),
                    text,
                    needs_parens: false,
                    swizzle: None,
                }
            }
            Expr::Unary { op, operand, .. } => {
                let rewritten = self.rewrite_expr(operand);
                let text = self.rebuild(expr.span(), &[(operand.span(), rewritten.text.clone())]);
                let ty = match op {
                    expr::UnOp::Not => Type::Scalar(Scalar::Bool),
                    expr::UnOp::BitNot => match rewritten.ty {
                        Type::Scalar(Scalar::Int) => Type::Scalar(Scalar::Int),
                        Type::Scalar(Scalar::Uint) => Type::Scalar(Scalar::Uint),
                        _ => Type::Unknown,
                    },
                    _ => rewritten.ty,
                };
                Rewritten {
                    ty,
                    text,
                    needs_parens: true,
                    swizzle: None,
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let rewritten_lhs = self.rewrite_expr(lhs);
                let rewritten_rhs = self.rewrite_expr(rhs);
                let (rewritten_lhs, rewritten_rhs) =
                    self.apply_binary_rules(*op, rewritten_lhs, rewritten_rhs);
                let text = self.rebuild(
                    expr.span(),
                    &[
                        (lhs.span(), rewritten_lhs.text.clone()),
                        (rhs.span(), rewritten_rhs.text.clone()),
                    ],
                );
                Rewritten {
                    ty: binary_result_type(*op, rewritten_lhs.ty, rewritten_rhs.ty),
                    text,
                    needs_parens: true,
                    swizzle: None,
                }
            }
            Expr::Ternary {
                cond,
                then_expr,
                other,
                ..
            } => {
                let rewritten_cond = self.rewrite_expr(cond);
                let rewritten_then = self.rewrite_expr(then_expr);
                let rewritten_other = self.rewrite_expr(other);
                let text = self.rebuild(
                    expr.span(),
                    &[
                        (cond.span(), rewritten_cond.text),
                        (then_expr.span(), rewritten_then.text),
                        (other.span(), rewritten_other.text),
                    ],
                );
                let ty = if rewritten_then.ty == rewritten_other.ty {
                    rewritten_then.ty
                } else {
                    Type::Unknown
                };
                Rewritten {
                    ty,
                    text,
                    needs_parens: true,
                    swizzle: None,
                }
            }
            Expr::Paren { inner, .. } => {
                let rewritten = self.rewrite_expr(inner);
                let text = self.rebuild(expr.span(), &[(inner.span(), rewritten.text)]);
                Rewritten {
                    ty: rewritten.ty,
                    text,
                    needs_parens: false,
                    swizzle: None,
                }
            }
            Expr::Index { base, index, .. } => {
                let rewritten_base = self.rewrite_expr(base);
                let rewritten_index = self.rewrite_expr(index);
                let text = self.rebuild(
                    expr.span(),
                    &[
                        (base.span(), rewritten_base.text),
                        (index.span(), rewritten_index.text),
                    ],
                );
                let ty = if let Type::Vector(scalar, _) = rewritten_base.ty {
                    Type::Scalar(scalar)
                } else {
                    Type::Unknown
                };
                Rewritten {
                    ty,
                    text,
                    needs_parens: false,
                    swizzle: None,
                }
            }
            Expr::Error { .. } => Rewritten {
                text: self.text_of(expr),
                ty: Type::Unknown,
                needs_parens: false,
                swizzle: None,
            },
        }
    }

    /// The text of an expression in the source.
    fn text_of(&self, expr: &Expr) -> String {
        expr.span().text(self.source).to_string()
    }

    /// Rebuild an expression from the new text of its children.
    ///
    /// Everything between the children - the operators, the parentheses, the
    /// comments - is copied from the source, so an expression that is not
    /// changed stays byte for byte identical.
    fn rebuild(&self, span: Span, children: &[(Span, String)]) -> String {
        let original = span.text(self.source);
        if children.is_empty() {
            return original.to_string();
        }
        let mut result = String::with_capacity(original.len());
        let mut position = span.start;
        for (child_span, child_text) in children {
            if child_span.start < position || child_span.end > span.end {
                continue;
            }
            result.push_str(&self.source[position..child_span.start]);
            result.push_str(child_text);
            position = child_span.end;
        }
        result.push_str(&self.source[position..span.end]);
        result
    }

    /// Record a replacement, if the text actually changed.
    fn push_edit(&mut self, span: Span, text: String) {
        if text == span.text(self.source) {
            return;
        }
        self.edits.push(Edit { span, text });
    }

    /// What kind of symbol an operand refers to, for the log messages.
    fn symbol_kind_of_text(&self, text: &str) -> SymbolKind {
        match self.symbols.lookup(text) {
            Some(symbol) => symbol.kind,
            None => SymbolKind::Unknown,
        }
    }

    /// Apply the binary rules to the operands of an operation.
    fn apply_binary_rules(
        &self,
        op: expr::BinOp,
        lhs: Rewritten,
        rhs: Rewritten,
    ) -> (Rewritten, Rewritten) {
        let mut lhs = lhs;
        let mut rhs = rhs;

        for rule in fixups::BINARY_RULES {
            let outcome = {
                let args = BinaryArgs {
                    op,
                    lhs_text: &lhs.text,
                    lhs_type: lhs.ty,
                    rhs_text: &rhs.text,
                    rhs_type: rhs.ty,
                };
                (rule.apply)(&args)
            };
            let Some(outcome) = outcome else {
                continue;
            };
            let kind = match outcome.side {
                Side::Lhs => self.symbol_kind_of_text(&lhs.text),
                Side::Rhs => self.symbol_kind_of_text(&rhs.text),
            };
            log::debug!(
                "{}: rule `{}` truncates a {:?} operand to {} components. {}",
                self.function,
                rule.id.name(),
                kind,
                outcome.width,
                rule.doc
            );
            match outcome.side {
                Side::Lhs => {
                    if let Some((text, ty)) = fixups::truncate_text(&lhs, outcome.width) {
                        lhs.text = text;
                        lhs.ty = ty;
                    }
                }
                Side::Rhs => {
                    if let Some((text, ty)) = fixups::truncate_text(&rhs, outcome.width) {
                        rhs.text = text;
                        rhs.ty = ty;
                    }
                }
            }
            break;
        }
        (lhs, rhs)
    }

    /// Apply the assignment rules to the value of an assignment.
    fn apply_assignment_rules(&self, target_type: Type, value: Rewritten) -> Rewritten {
        for rule in fixups::ASSIGN_RULES {
            let outcome = {
                let args = AssignArgs {
                    target_type,
                    value_text: &value.text,
                    value_type: value.ty,
                };
                (rule.apply)(&args)
            };
            let Some(outcome) = outcome else {
                continue;
            };
            log::debug!(
                "{}: rule `{}` truncates a value to {} components. {}",
                self.function,
                rule.id.name(),
                outcome.width,
                rule.doc
            );
            if let Some((text, ty)) = fixups::truncate_text(&value, outcome.width) {
                return Rewritten { text, ty, ..value };
            }
        }
        value
    }

    /// Apply the rules to the value of a compound assignment.
    ///
    /// The target of a compound assignment cannot be truncated - it is an
    /// l-value - so only a value that is longer than the target is
    /// truncated.  Everything else is left alone.
    fn apply_compound_rules(&self, target_type: Type, value: Rewritten) -> Rewritten {
        let Type::Vector(_, target) = target_type else {
            return value;
        };
        let Type::Vector(_, value_width) = value.ty else {
            return value;
        };
        if value_width <= target {
            return value;
        }
        if let Some((text, ty)) = fixups::truncate_text(&value, target) {
            return Rewritten { text, ty, ..value };
        }
        value
    }
}

#[cfg(test)]
mod tests;
