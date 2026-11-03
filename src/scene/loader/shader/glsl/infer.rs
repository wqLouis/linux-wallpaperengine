//! Type inference for expressions.
//!
//! The inference is deliberately conservative: whenever it cannot work out
//! the type of an expression it returns [`Type::Unknown`], and the
//! normalisation rules never rewrite an expression whose type is unknown.
//! Guessing would change shaders that work today, which is far worse than
//! leaving a broken shader broken.

use super::expr::{BinOp, Expr, UnOp};
use super::symbols::SymbolTable;
use super::types::{Scalar, Type};

/// How the type of a built-in function is determined.
#[derive(Debug, Clone, Copy)]
enum RetRule {
    /// The type of the argument at the given index.
    Arg(usize),
    /// A fixed type.
    Fixed(Type),
    /// The type of the first argument.
    LikeArg0,
    /// The matrix with its columns and rows swapped.
    Transpose,
    /// A boolean vector with the shape of the first argument.
    BoolVector,
    /// The result of an HLSL style `mul`.
    Mul,
}

/// A built-in function.
#[derive(Debug, Clone, Copy)]
struct Builtin {
    /// The name of the function.
    name: &'static str,
    /// The smallest number of arguments it accepts.
    min_args: usize,
    /// The largest number of arguments it accepts.
    max_args: usize,
    /// How its result type is determined.
    ret: RetRule,
}

/// The built-in functions the preprocessor knows about.
const BUILTINS: &[Builtin] = &[
    // Constructors are handled before this table is consulted.
    Builtin {
        name: "abs",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "sign",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "floor",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "ceil",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "trunc",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "round",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "roundEven",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "fract",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "mod",
        min_args: 1,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "min",
        min_args: 2,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "max",
        min_args: 2,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "clamp",
        min_args: 3,
        max_args: 3,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "mix",
        min_args: 3,
        max_args: 3,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "step",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Arg(1),
    },
    Builtin {
        name: "smoothstep",
        min_args: 3,
        max_args: 3,
        ret: RetRule::Arg(2),
    },
    Builtin {
        name: "pow",
        min_args: 2,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "sqrt",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "inverseSqrt",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "exp",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "log",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "exp2",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "log2",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "sin",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "cos",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "tan",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "asin",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "acos",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "sinh",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "cosh",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "tanh",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "radians",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "degrees",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "normalize",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "reflect",
        min_args: 2,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "refract",
        min_args: 3,
        max_args: 3,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "faceforward",
        min_args: 3,
        max_args: 3,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "cross",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 3)),
    },
    Builtin {
        name: "length",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "distance",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "dot",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "determinant",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "atan",
        min_args: 1,
        max_args: 2,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "fma",
        min_args: 3,
        max_args: 3,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "ldexp",
        min_args: 2,
        max_args: 2,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "transpose",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Transpose,
    },
    Builtin {
        name: "inverse",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "mul",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Mul,
    },
    Builtin {
        name: "dFdx",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "dFdy",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "fwidth",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "saturate",
        min_args: 1,
        max_args: 1,
        ret: RetRule::LikeArg0,
    },
    Builtin {
        name: "float",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Float)),
    },
    Builtin {
        name: "int",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Int)),
    },
    Builtin {
        name: "uint",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Uint)),
    },
    Builtin {
        name: "bool",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Bool)),
    },
    Builtin {
        name: "any",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Bool)),
    },
    Builtin {
        name: "all",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Bool)),
    },
    Builtin {
        name: "not",
        min_args: 1,
        max_args: 1,
        ret: RetRule::Fixed(Type::Scalar(Scalar::Bool)),
    },
    Builtin {
        name: "isnan",
        min_args: 1,
        max_args: 1,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "isinf",
        min_args: 1,
        max_args: 1,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "lessThan",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "lessThanEqual",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "greaterThan",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "greaterThanEqual",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "equal",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "notEqual",
        min_args: 2,
        max_args: 2,
        ret: RetRule::BoolVector,
    },
    Builtin {
        name: "texture",
        min_args: 2,
        max_args: 4,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "textureLod",
        min_args: 2,
        max_args: 4,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "textureProj",
        min_args: 2,
        max_args: 3,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "textureGrad",
        min_args: 3,
        max_args: 4,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "textureOffset",
        min_args: 3,
        max_args: 4,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "texelFetch",
        min_args: 2,
        max_args: 3,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "texture2D",
        min_args: 2,
        max_args: 2,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
    Builtin {
        name: "texture2DLod",
        min_args: 3,
        max_args: 3,
        ret: RetRule::Fixed(Type::Vector(Scalar::Float, 4)),
    },
];

/// The type of a call to `name` with the given argument types.
pub fn call_result_type(name: &str, arg_types: &[Type]) -> Type {
    // A constructor always has the type its name denotes.
    if let Some(ty) = super::types::parse_type_name(name) {
        return ty;
    }
    for builtin in BUILTINS {
        if builtin.name == name
            && arg_types.len() >= builtin.min_args
            && arg_types.len() <= builtin.max_args
        {
            return apply_ret_rule(builtin.ret, arg_types);
        }
    }
    Type::Unknown
}

/// Apply a result rule to the argument types of a call.
fn apply_ret_rule(rule: RetRule, arg_types: &[Type]) -> Type {
    match rule {
        RetRule::Arg(index) => arg_types.get(index).copied().unwrap_or(Type::Unknown),
        RetRule::Fixed(ty) => ty,
        RetRule::LikeArg0 => arg_types.first().copied().unwrap_or(Type::Unknown),
        RetRule::Transpose => {
            if let Some(Type::Matrix(cols, rows)) = arg_types.first().copied() {
                Type::Matrix(rows, cols)
            } else {
                Type::Unknown
            }
        }
        RetRule::BoolVector => {
            if let Some(Type::Vector(_, n)) = arg_types.first().copied() {
                Type::Vector(Scalar::Bool, n)
            } else if let Some(Type::Scalar(_)) = arg_types.first().copied() {
                Type::Scalar(Scalar::Bool)
            } else {
                Type::Unknown
            }
        }
        RetRule::Mul => mul_result(arg_types),
    }
}

/// The result type of an HLSL style `mul(a, b)`.
fn mul_result(arg_types: &[Type]) -> Type {
    match (
        arg_types.first().copied().unwrap_or(Type::Unknown),
        arg_types.get(1).copied().unwrap_or(Type::Unknown),
    ) {
        (Type::Matrix(cols, rows), Type::Vector(_, n)) if n == cols => {
            Type::Vector(Scalar::Float, rows)
        }
        (Type::Vector(_, n), Type::Matrix(cols, rows)) if n == rows => {
            Type::Vector(Scalar::Float, cols)
        }
        (Type::Matrix(cols1, rows1), Type::Matrix(cols2, rows2)) if cols1 == rows2 => {
            Type::Matrix(cols2, rows1)
        }
        (Type::Vector(scalar, n), Type::Vector(_, _)) => Type::Vector(scalar, n),
        _ => Type::Unknown,
    }
}

/// The index a swizzle component refers to.
fn swizzle_index(component: u8) -> Option<u8> {
    match component {
        b'x' | b'r' | b's' => Some(0),
        b'y' | b'g' | b't' => Some(1),
        b'z' | b'b' | b'p' => Some(2),
        b'w' | b'a' | b'q' => Some(3),
        _ => None,
    }
}

/// The type of a swizzle such as `value.xy`.
pub fn swizzle_type(base: Type, comps: &str) -> Type {
    let Type::Vector(count, _) = base else {
        return Type::Unknown;
    };
    let Some(width) = base.components() else {
        return Type::Unknown;
    };
    if comps.is_empty() || comps.len() > 4 {
        return Type::Unknown;
    }
    for component in comps.bytes() {
        let Some(index) = swizzle_index(component) else {
            return Type::Unknown;
        };
        if index >= width {
            return Type::Unknown;
        }
    }
    if comps.len() == 1 {
        Type::Scalar(count)
    } else {
        Type::Vector(count, comps.len() as u8)
    }
}

/// The type of a binary operation.
pub fn binary_result_type(op: BinOp, lhs: Type, rhs: Type) -> Type {
    match op {
        BinOp::Less
        | BinOp::Greater
        | BinOp::LessEq
        | BinOp::GreaterEq
        | BinOp::Equal
        | BinOp::NotEqual
        | BinOp::LogAnd
        | BinOp::LogOr => Type::Scalar(Scalar::Bool),
        _ => arithmetic_result_type(lhs, rhs),
    }
}

/// The type of an arithmetic operation.
fn arithmetic_result_type(lhs: Type, rhs: Type) -> Type {
    match (lhs, rhs) {
        // A matrix times a vector.
        (Type::Matrix(cols, rows), Type::Vector(_, n)) if n == cols => {
            Type::Vector(Scalar::Float, rows)
        }
        // A vector times a matrix.
        (Type::Vector(_, n), Type::Matrix(cols, rows)) if n == rows => {
            Type::Vector(Scalar::Float, cols)
        }
        // A matrix times a matrix.
        (Type::Matrix(cols1, rows1), Type::Matrix(cols2, rows2)) if cols1 == rows2 => {
            Type::Matrix(cols2, rows1)
        }
        // Two vectors: the result has as many components as the shorter one.
        // (The operands are truncated to that width by the normalisation
        // rules before the operation is carried out.)
        (Type::Vector(scalar, n), Type::Vector(_, m)) => Type::Vector(scalar, n.min(m)),
        // A scalar and a vector: the scalar is broadcast.
        (Type::Vector(scalar, n), Type::Scalar(_)) => Type::Vector(scalar, n),
        (Type::Scalar(_), Type::Vector(scalar, m)) => Type::Vector(scalar, m),
        // Two scalars.
        (Type::Scalar(a), Type::Scalar(b)) => Type::Scalar(a.promote(b)),
        _ => Type::Unknown,
    }
}

/// The type of an expression.
pub fn infer(expr: &Expr, symbols: &SymbolTable) -> Type {
    match expr {
        Expr::Literal { ty, .. } => *ty,
        Expr::Ident { name, .. } => symbols.lookup_type(name).unwrap_or(Type::Unknown),
        Expr::Swizzle { base, comps, .. } => swizzle_type(infer(base, symbols), comps),
        Expr::Call { name, args, .. } => {
            let arg_types: Vec<Type> = args.iter().map(|arg| infer(arg, symbols)).collect();
            call_result_type(name, &arg_types)
        }
        Expr::Unary { op, operand, .. } => {
            let operand_type = infer(operand, symbols);
            match op {
                UnOp::Plus | UnOp::Minus => operand_type,
                UnOp::Not => Type::Scalar(Scalar::Bool),
                UnOp::BitNot => match operand_type {
                    Type::Scalar(Scalar::Int) => Type::Scalar(Scalar::Int),
                    Type::Scalar(Scalar::Uint) => Type::Scalar(Scalar::Uint),
                    _ => Type::Unknown,
                },
                UnOp::IncDec => operand_type,
            }
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            binary_result_type(*op, infer(lhs, symbols), infer(rhs, symbols))
        }
        Expr::Ternary {
            then_expr, other, ..
        } => {
            let then_type = infer(then_expr, symbols);
            let other_type = infer(other, symbols);
            if then_type == other_type {
                then_type
            } else {
                Type::Unknown
            }
        }
        Expr::Paren { inner, .. } => infer(inner, symbols),
        Expr::Index { base, .. } => {
            let base_type = infer(base, symbols);
            if is_array(base, symbols) {
                // The element of an array has the type of the array.
                base_type
            } else if let Type::Vector(scalar, _) = base_type {
                Type::Scalar(scalar)
            } else {
                Type::Unknown
            }
        }
        Expr::Error { .. } => Type::Unknown,
    }
}

/// Whether an expression refers to an array.
fn is_array(expr: &Expr, symbols: &SymbolTable) -> bool {
    match expr {
        Expr::Ident { name, .. } => symbols
            .lookup(name)
            .map(|symbol| symbol.is_array)
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_have_their_own_type() {
        assert_eq!(
            call_result_type("vec4", &[]),
            Type::Vector(Scalar::Float, 4)
        );
        assert_eq!(
            call_result_type("vec3", &[]),
            Type::Vector(Scalar::Float, 3)
        );
    }

    #[test]
    fn builtins_use_their_argument_types() {
        let vec3 = Type::Vector(Scalar::Float, 3);
        let float = Type::Scalar(Scalar::Float);
        assert_eq!(call_result_type("clamp", &[vec3, float, float]), vec3);
        assert_eq!(call_result_type("mix", &[vec3, vec3, float]), vec3);
        assert_eq!(call_result_type("step", &[float, vec3]), vec3);
        assert_eq!(call_result_type("smoothstep", &[float, float, vec3]), vec3);
        assert_eq!(
            call_result_type("dot", &[vec3, vec3]),
            Type::Scalar(Scalar::Float)
        );
        assert_eq!(call_result_type("normalize", &[vec3]), vec3);
        assert_eq!(call_result_type("unknownFunction", &[vec3]), Type::Unknown);
    }

    #[test]
    fn mul_follows_the_hlsl_rules() {
        let mat4 = Type::Matrix(4, 4);
        let vec4 = Type::Vector(Scalar::Float, 4);
        assert_eq!(call_result_type("mul", &[mat4, vec4]), vec4);
        assert_eq!(call_result_type("mul", &[vec4, mat4]), vec4);
        assert_eq!(call_result_type("mul", &[mat4, mat4]), mat4);
    }

    #[test]
    fn mismatched_vectors_take_the_shorter_width() {
        let vec4 = Type::Vector(Scalar::Float, 4);
        let vec2 = Type::Vector(Scalar::Float, 2);
        assert_eq!(
            binary_result_type(BinOp::Mul, vec4, vec2),
            Type::Vector(Scalar::Float, 2)
        );
        assert_eq!(
            binary_result_type(BinOp::Mul, vec2, vec4),
            Type::Vector(Scalar::Float, 2)
        );
    }

    #[test]
    fn scalars_are_broadcast_legally() {
        let vec4 = Type::Vector(Scalar::Float, 4);
        let float = Type::Scalar(Scalar::Float);
        assert_eq!(binary_result_type(BinOp::Mul, vec4, float), vec4);
        assert_eq!(binary_result_type(BinOp::Mul, float, vec4), vec4);
    }
}
