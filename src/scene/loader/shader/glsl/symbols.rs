//! The symbol table of the type-aware preprocessor.
//!
//! The table holds the types of everything a shader can refer to: the
//! uniforms of the effect (their types come from the layout that was
//! collected when the scene was loaded), the attributes and varyings of the
//! shader, the locals and constants of a function, and the built-in globals
//! of GLSL.
//!
//! Scoping is function local: a function's parameters and locals live in a
//! scope that is pushed when the function is entered and popped when it is
//! left.  Global declarations live in the outermost scope.

use crate::collections::{Map, new_map};

use super::types::{Scalar, Type};

/// What a symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A uniform of the effect.
    Uniform,
    /// A vertex attribute.
    Attribute,
    /// A varying (an `in` or an `out` of the shader interface).
    Varying,
    /// A local variable.
    Local,
    /// A constant.
    Constant,
    /// A function parameter.
    Parameter,
    /// A built-in global of GLSL.
    Builtin,
    /// A symbol the preprocessor could not classify.
    Unknown,
}

/// A named symbol.
#[derive(Debug, Clone)]
pub struct Symbol {
    /// The type of the symbol.
    ///
    /// For an array this is the type of its elements.
    pub ty: Type,
    /// What kind of symbol this is.
    pub kind: SymbolKind,
    /// Whether the symbol is an array.
    ///
    /// The type of an array symbol is the type of its elements, so the
    /// difference between `vec2 v;` and `vec2 v[4];` only shows up when the
    /// symbol is indexed.
    pub is_array: bool,
}
/// The built-in globals every GLSL shader may use.
const BUILTIN_GLOBALS: &[(&str, Type)] = &[
    ("gl_Position", Type::Vector(Scalar::Float, 4)),
    ("gl_PointSize", Type::Scalar(Scalar::Float)),
    ("gl_FragCoord", Type::Vector(Scalar::Float, 4)),
    ("gl_FrontFacing", Type::Scalar(Scalar::Bool)),
    ("gl_FragDepth", Type::Scalar(Scalar::Float)),
    ("gl_PointCoord", Type::Vector(Scalar::Float, 2)),
    ("gl_VertexID", Type::Scalar(Scalar::Int)),
    ("gl_InstanceID", Type::Scalar(Scalar::Int)),
    ("gl_FragColor", Type::Vector(Scalar::Float, 4)),
    ("gl_DepthRange", Type::Vector(Scalar::Float, 4)),
    ("gl_WorkGroupSize", Type::Vector(Scalar::Uint, 3)),
    ("gl_GlobalInvocationID", Type::Vector(Scalar::Uint, 3)),
    ("gl_LocalInvocationID", Type::Vector(Scalar::Uint, 3)),
    ("gl_NumWorkGroups", Type::Vector(Scalar::Uint, 3)),
    ("gl_WorkGroupID", Type::Vector(Scalar::Uint, 3)),
];

/// The symbols that are in scope.
#[derive(Debug)]
pub struct SymbolTable {
    scopes: Vec<Map<String, Symbol>>,
}

impl SymbolTable {
    /// A table with the built-in globals in its outermost scope.
    pub fn new() -> Self {
        let mut global = new_map();
        for (name, ty) in BUILTIN_GLOBALS {
            global.insert(
                (*name).to_string(),
                Symbol {
                    ty: *ty,
                    kind: SymbolKind::Builtin,
                    is_array: false,
                },
            );
        }
        Self {
            scopes: vec![global],
        }
    }

    /// Push a new scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(new_map());
    }

    /// Pop the innermost scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Declare a symbol in the innermost scope.
    pub fn declare(&mut self, name: &str, ty: Type, kind: SymbolKind) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                Symbol {
                    ty,
                    kind,
                    is_array: false,
                },
            );
        }
    }

    /// Declare a symbol in the outermost scope.
    ///
    /// A symbol that is already known keeps its type when the new type is
    /// unknown: the layout knows the type of a varying that is declared as
    /// an array, the parser only knows that it is an array.
    pub fn declare_global(&mut self, name: &str, ty: Type, kind: SymbolKind, is_array: bool) {
        let Some(scope) = self.scopes.first_mut() else {
            return;
        };
        match scope.get_mut(name) {
            Some(symbol) => {
                if ty != Type::Unknown {
                    symbol.ty = ty;
                }
                symbol.is_array = symbol.is_array || is_array;
            }
            None => {
                scope.insert(name.to_string(), Symbol { ty, kind, is_array });
            }
        }
    }

    /// Look a symbol up, innermost scope first.
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol);
            }
        }
        None
    }

    /// The type of a symbol, if it is known.
    pub fn lookup_type(&self, name: &str) -> Option<Type> {
        self.lookup(name).map(|symbol| symbol.ty)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_known() {
        let table = SymbolTable::new();
        assert_eq!(
            table.lookup_type("gl_Position"),
            Some(Type::Vector(Scalar::Float, 4))
        );
        assert!(table.lookup_type("gl_FragColor").is_some());
    }

    #[test]
    fn inner_shadows_outer_scopes() {
        let mut table = SymbolTable::new();
        table.declare_global(
            "x",
            Type::Vector(Scalar::Float, 4),
            SymbolKind::Uniform,
            false,
        );
        table.push_scope();
        table.declare("x", Type::Vector(Scalar::Float, 2), SymbolKind::Local);
        assert_eq!(table.lookup_type("x"), Some(Type::Vector(Scalar::Float, 2)));
        table.pop_scope();
        assert_eq!(table.lookup_type("x"), Some(Type::Vector(Scalar::Float, 4)));
    }
}
