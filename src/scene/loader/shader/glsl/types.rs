//! The type model of the type-aware preprocessor.
//!
//! Only the types the normalisation rules care about are modelled: scalars,
//! vectors and matrices.  Everything else (samplers, structs, arrays) is
//! either [`Type::Opaque`] - a type that is known but carries no component
//! count - or [`Type::Unknown`], which means "the preprocessor could not
//! work this out".  Rules must never rewrite an expression whose type is
//! unknown.

/// A scalar type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar {
    /// `float`.
    Float,
    /// `int`.
    Int,
    /// `uint`.
    Uint,
    /// `bool`.
    Bool,
}

impl Scalar {
    /// The scalar type a binary operation between `self` and `other` yields.
    pub fn promote(self, other: Scalar) -> Scalar {
        if self == Scalar::Float || other == Scalar::Float {
            Scalar::Float
        } else if self == Scalar::Uint || other == Scalar::Uint {
            Scalar::Uint
        } else if self == Scalar::Int || other == Scalar::Int {
            Scalar::Int
        } else {
            Scalar::Bool
        }
    }
}

/// A GLSL type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// A scalar.
    Scalar(Scalar),
    /// A vector of `n` components.
    Vector(Scalar, u8),
    /// A matrix with `cols` columns and `rows` rows.
    Matrix(u8, u8),
    /// A type that is not a number, such as a sampler or a struct.
    Opaque,
    /// A type the preprocessor could not determine.
    Unknown,
}

impl Type {
    /// The number of components, if the type has any.
    ///
    /// A scalar has one component, a vector has as many as it has
    /// components.  Matrices, opaque types and unknown types do not have a
    /// component count.
    pub fn components(self) -> Option<u8> {
        match self {
            Type::Scalar(_) => Some(1),
            Type::Vector(_, n) => Some(n),
            _ => None,
        }
    }
}

/// The swizzle that keeps the first `n` components of a vector.
///
/// `n == 1` yields a scalar, which is why the swizzle is `x` and not `xxx`.
pub fn swizzle_for(n: u8) -> Option<&'static str> {
    match n {
        1 => Some("x"),
        2 => Some("xy"),
        3 => Some("xyz"),
        4 => Some("xyzw"),
        _ => None,
    }
}

/// The type a GLSL type name denotes.
///
/// `None` means "not a type name the preprocessor knows about".
pub fn parse_type_name(name: &str) -> Option<Type> {
    let ty = match name {
        "float" | "double" => Type::Scalar(Scalar::Float),
        "int" => Type::Scalar(Scalar::Int),
        "uint" => Type::Scalar(Scalar::Uint),
        "bool" => Type::Scalar(Scalar::Bool),
        "vec2" => Type::Vector(Scalar::Float, 2),
        "vec3" => Type::Vector(Scalar::Float, 3),
        "vec4" => Type::Vector(Scalar::Float, 4),
        "ivec2" => Type::Vector(Scalar::Int, 2),
        "ivec3" => Type::Vector(Scalar::Int, 3),
        "ivec4" => Type::Vector(Scalar::Int, 4),
        "uvec2" => Type::Vector(Scalar::Uint, 2),
        "uvec3" => Type::Vector(Scalar::Uint, 3),
        "uvec4" => Type::Vector(Scalar::Uint, 4),
        "bvec2" => Type::Vector(Scalar::Bool, 2),
        "bvec3" => Type::Vector(Scalar::Bool, 3),
        "bvec4" => Type::Vector(Scalar::Bool, 4),
        "mat2" => Type::Matrix(2, 2),
        "mat3" => Type::Matrix(3, 3),
        "mat4" => Type::Matrix(4, 4),
        "mat2x2" => Type::Matrix(2, 2),
        "mat2x3" => Type::Matrix(2, 3),
        "mat2x4" => Type::Matrix(2, 4),
        "mat3x2" => Type::Matrix(3, 2),
        "mat3x3" => Type::Matrix(3, 3),
        "mat3x4" => Type::Matrix(3, 4),
        "mat4x2" => Type::Matrix(4, 2),
        "mat4x3" => Type::Matrix(4, 3),
        "mat4x4" => Type::Matrix(4, 4),
        "sampler" | "sampler2D" | "sampler3D" | "samplerCube" | "sampler2DShadow" | "texture2D"
        | "texture3D" | "textureCube" | "image2D" | "usampler2D" => Type::Opaque,
        _ => return None,
    };
    Some(ty)
}

/// The type a uniform declaration string denotes.
///
/// The declarations collected by the layout look like `vec4`, `mat4` or
/// `float [32]`; array types are not modelled and therefore reported as
/// [`Type::Unknown`].
pub fn parse_declared_type(declared: &str) -> Type {
    let base = declared.trim();
    if base.contains('[') {
        return Type::Unknown;
    }
    parse_type_name(base).unwrap_or(Type::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_counts() {
        assert_eq!(Type::Scalar(Scalar::Float).components(), Some(1));
        assert_eq!(Type::Vector(Scalar::Float, 4).components(), Some(4));
        assert_eq!(Type::Matrix(4, 4).components(), None);
        assert_eq!(Type::Unknown.components(), None);
    }

    #[test]
    fn swizzles() {
        assert_eq!(swizzle_for(1), Some("x"));
        assert_eq!(swizzle_for(2), Some("xy"));
        assert_eq!(swizzle_for(3), Some("xyz"));
        assert_eq!(swizzle_for(4), Some("xyzw"));
        assert_eq!(swizzle_for(0), None);
        assert_eq!(swizzle_for(5), None);
    }

    #[test]
    fn declared_types() {
        assert_eq!(parse_declared_type("vec4"), Type::Vector(Scalar::Float, 4));
        assert_eq!(parse_declared_type("mat4"), Type::Matrix(4, 4));
        // Arrays are not modelled.
        assert_eq!(parse_declared_type("float [32]"), Type::Unknown);
        assert_eq!(parse_declared_type("nonsense"), Type::Unknown);
    }
}
