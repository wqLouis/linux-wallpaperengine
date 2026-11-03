//! The generic normalisation rules.
//!
//! Wallpaper Engine's own shader compiler is more lenient than the GLSL
//! specification: where strict GLSL rejects an operation because two
//! operands have a different number of components, the reference engine
//! truncates the longer vector to the number of components of the shorter
//! one.  `vec4 * vec2` therefore behaves like `vec4.xy * vec2`.
//!
//! The rules below reproduce that behaviour generically.  They are
//! deliberately table driven: adding a rule means adding an entry to one of
//! the tables, the parser does not have to change.
//!
//! Two rules are implemented:
//!
//! * [`RuleId::BinaryOperandTruncation`] truncates the longer operand of a
//!   binary operation to the component count of the shorter one.
//! * [`RuleId::AssignmentTruncation`] truncates the value of an assignment
//!   to the component count of the target.
//!
//! Both rules only ever fire when the types involved are known.  An
//! expression whose type could not be determined is left alone: a false
//! positive that changes a working shader is worse than the bug the rules
//! fix.

use super::expr::BinOp;
use super::types::{self, Type};

/// The identifier of a normalisation rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleId {
    /// Truncate the longer operand of a binary operation.
    BinaryOperandTruncation,
    /// Truncate the value of an assignment to the width of the target.
    AssignmentTruncation,
}

impl RuleId {
    /// A short, stable name of the rule.
    pub fn name(self) -> &'static str {
        match self {
            RuleId::BinaryOperandTruncation => "binary-operand-truncation",
            RuleId::AssignmentTruncation => "assignment-truncation",
        }
    }
}

/// Which operand of a binary operation a rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The left hand side.
    Lhs,
    /// The right hand side.
    Rhs,
}

/// The arguments a binary rule is given.
#[derive(Debug)]
pub struct BinaryArgs<'a> {
    /// The operator of the operation.
    pub op: BinOp,
    /// The text of the left hand side.
    pub lhs_text: &'a str,
    /// The type of the left hand side.
    pub lhs_type: Type,
    /// The text of the right hand side.
    pub rhs_text: &'a str,
    /// The type of the right hand side.
    pub rhs_type: Type,
}

/// What a binary rule decided.
#[derive(Debug, Clone, Copy)]
pub struct BinaryOutcome {
    /// The operand that has to be truncated.
    pub side: Side,
    /// The number of components it has to be truncated to.
    pub width: u8,
}

/// A binary rule.
pub struct BinaryRule {
    /// The identifier of the rule.
    pub id: RuleId,
    /// What the rule does.
    pub doc: &'static str,
    /// The rule itself.
    pub apply: fn(&BinaryArgs<'_>) -> Option<BinaryOutcome>,
}

/// The binary rules, in the order they are applied.
pub const BINARY_RULES: &[BinaryRule] = &[BinaryRule {
    id: RuleId::BinaryOperandTruncation,
    doc: "Truncate the longer vector operand of a binary operation to the \
          component count of the shorter one, so that `vec4 * vec2` behaves \
          like `vec4.xy * vec2`.",
    apply: truncate_longer_operand,
}];

/// The arguments an assignment rule is given.
#[derive(Debug)]
pub struct AssignArgs<'a> {
    /// The type of the assignment target.
    pub target_type: Type,
    /// The text of the value.
    pub value_text: &'a str,
    /// The type of the value.
    pub value_type: Type,
}

/// What an assignment rule decided.
#[derive(Debug, Clone, Copy)]
pub struct AssignOutcome {
    /// The number of components the value has to be truncated to.
    pub width: u8,
}

/// An assignment rule.
pub struct AssignRule {
    /// The identifier of the rule.
    pub id: RuleId,
    /// What the rule does.
    pub doc: &'static str,
    /// The rule itself.
    pub apply: fn(&AssignArgs<'_>) -> Option<AssignOutcome>,
}

/// The assignment rules, in the order they are applied.
pub const ASSIGN_RULES: &[AssignRule] = &[AssignRule {
    id: RuleId::AssignmentTruncation,
    doc: "Truncate a vector value to the component count of the assignment \
          target, so that assigning a `vec4` to a `vec2` becomes \
          `target = value.xy;`.",
    apply: truncate_value_to_target,
}];

/// Truncate the longer operand of a binary operation.
///
/// The rule only fires when both operands are vectors of a different width.
/// A scalar mixed with a vector is legal GLSL and is left alone.
fn truncate_longer_operand(args: &BinaryArgs<'_>) -> Option<BinaryOutcome> {
    let Type::Vector(_, lhs) = args.lhs_type else {
        return None;
    };
    let Type::Vector(_, rhs) = args.rhs_type else {
        return None;
    };
    if lhs == rhs {
        return None;
    }
    if lhs > rhs {
        log::trace!(
            "{:?}: truncating `{}` to the {} components of `{}`",
            args.op,
            args.lhs_text,
            rhs,
            args.rhs_text
        );
        Some(BinaryOutcome {
            side: Side::Lhs,
            width: rhs,
        })
    } else {
        log::trace!(
            "{:?}: truncating `{}` to the {} components of `{}`",
            args.op,
            args.rhs_text,
            lhs,
            args.lhs_text
        );
        Some(BinaryOutcome {
            side: Side::Rhs,
            width: lhs,
        })
    }
}

/// Truncate the value of an assignment to the width of the target.
///
/// A scalar assigned to a vector is legal GLSL (the scalar is broadcast) and
/// is left alone, and so is a value that is not longer than the target.
fn truncate_value_to_target(args: &AssignArgs<'_>) -> Option<AssignOutcome> {
    let target = match args.target_type {
        Type::Vector(_, n) => n,
        Type::Scalar(_) => 1,
        _ => return None,
    };
    let Type::Vector(_, value) = args.value_type else {
        return None;
    };
    if value <= target {
        return None;
    }
    log::trace!(
        "truncating `{}` to the {} components of the target",
        args.value_text,
        target
    );
    Some(AssignOutcome { width: target })
}

/// A swizzle and the text of the expression it is applied to.
#[derive(Debug, Clone)]
pub struct SwizzleInfo {
    /// The text of the expression the swizzle is applied to.
    pub base: String,
    /// The components of the swizzle.
    pub comps: String,
}

/// The rewritten form of an expression, as the rules see it.
#[derive(Debug, Clone)]
pub struct Rewritten {
    /// The text of the expression.
    pub text: String,
    /// The type of the expression.
    pub ty: Type,
    /// Whether the expression has to be parenthesised before a swizzle can
    /// be appended to it.
    pub needs_parens: bool,
    /// The swizzle the expression is, if it is one.
    pub swizzle: Option<SwizzleInfo>,
}

/// Truncate `rewritten` to `width` components.
///
/// The text is a vector of a known width; the result is the same text with a
/// swizzle appended to it, parenthesised first when the text is not
/// something that can be swizzled directly.
///
/// A swizzle is truncated by shortening its own components rather than by
/// appending another swizzle: `a.xyxy` truncated to two components is
/// `a.xy`, not `a.xyxy.xy`, which would not be a valid swizzle.
pub fn truncate_text(rewritten: &Rewritten, width: u8) -> Option<(String, Type)> {
    let Type::Vector(scalar, count) = rewritten.ty else {
        return None;
    };
    if count <= width {
        return None;
    }
    let swizzle = types::swizzle_for(width)?;

    if let Some(info) = &rewritten.swizzle
        && info.comps.chars().count() > width as usize
    {
        let comps: String = info.comps.chars().take(width as usize).collect();
        let truncated = format!("{}.{}", info.base, comps);
        return Some((truncated, Type::Vector(scalar, width)));
    }

    let truncated = if rewritten.needs_parens {
        format!("({}).{}", rewritten.text, swizzle)
    } else {
        format!("{}.{}", rewritten.text, swizzle)
    };
    Some((truncated, Type::Vector(scalar, width)))
}

#[cfg(test)]
mod tests {
    use super::types::Scalar;
    use super::*;

    fn binary_args(lhs_type: Type, rhs_type: Type) -> BinaryArgs<'static> {
        BinaryArgs {
            op: BinOp::Mul,
            lhs_text: "a",
            lhs_type,
            rhs_text: "b",
            rhs_type,
        }
    }

    #[test]
    fn the_longer_operand_is_truncated() {
        let args = binary_args(
            Type::Vector(Scalar::Float, 4),
            Type::Vector(Scalar::Float, 2),
        );
        let outcome = truncate_longer_operand(&args).expect("the rule fires");
        assert_eq!(outcome.side, Side::Lhs);
        assert_eq!(outcome.width, 2);
    }

    #[test]
    fn scalars_are_left_alone() {
        let args = binary_args(Type::Vector(Scalar::Float, 4), Type::Scalar(Scalar::Float));
        assert!(truncate_longer_operand(&args).is_none());
    }

    #[test]
    fn unknown_types_are_left_alone() {
        let args = binary_args(Type::Unknown, Type::Vector(Scalar::Float, 2));
        assert!(truncate_longer_operand(&args).is_none());
    }

    #[test]
    fn assignments_are_truncated_to_the_target() {
        let args = AssignArgs {
            target_type: Type::Vector(Scalar::Float, 2),
            value_text: "a",
            value_type: Type::Vector(Scalar::Float, 4),
        };
        let outcome = truncate_value_to_target(&args).expect("the rule fires");
        assert_eq!(outcome.width, 2);
    }

    #[test]
    fn widening_is_left_alone() {
        let args = AssignArgs {
            target_type: Type::Vector(Scalar::Float, 4),
            value_text: "a",
            value_type: Type::Vector(Scalar::Float, 2),
        };
        assert!(truncate_value_to_target(&args).is_none());
    }
}
