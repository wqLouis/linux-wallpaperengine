//! The varyings a shader writes to.
//!
//! A varying of the fragment shader is an input: Vulkan GLSL does not allow
//! the fragment shader to write to it.  The shaders of Wallpaper Engine do
//! it anyway, so the pipeline has to know which varyings the fragment
//! shader writes to: those are renamed and replaced by a local variable.
//!
//! The detection is done with the parser of the type aware stage
//! ([`super::glsl`]): the shader is parsed, the statements are walked and
//! the assignments to a varying are found.  A text search would find the
//! wrong things: the name of a varying can be the tail of the name of
//! another one (`v_UV` and `my_v_UV`), and a write can be written without
//! spaces around the `=`.

use crate::collections::Set;

use super::glsl::expr::{Expr, UnOp};
use super::glsl::parser::{self, Block, Stmt};
use super::layout::EffectLayout;

/// The names of the varyings the source writes to.
///
/// Only the names of `candidates` are reported: the candidates are the
/// varyings of the shader, the source is the fragment shader of an effect.
pub fn written_varyings(source: &str, candidates: &Set<String>) -> Vec<String> {
    let unit = parser::parse(source);
    let mut written: Vec<String> = Vec::new();
    for function in &unit.functions {
        collect_block(&function.body, candidates, &mut written);
    }
    written
}

/// Collect the varyings the statements of a block write to.
fn collect_block(block: &Block, candidates: &Set<String>, written: &mut Vec<String>) {
    for stmt in &block.stmts {
        collect_stmt(stmt, candidates, written);
    }
}

/// Collect the varyings a statement writes to.
fn collect_stmt(stmt: &Stmt, candidates: &Set<String>, written: &mut Vec<String>) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            // The target of an assignment is written to.
            if let Some(name) = variable_of(target) {
                record(name, candidates, written);
            }
            collect_expr(value, candidates, written);
        }
        Stmt::Expr(expr) => collect_expr(expr, candidates, written),
        Stmt::If {
            then_block,
            else_block,
            ..
        } => {
            collect_block(then_block, candidates, written);
            if let Some(block) = else_block {
                collect_block(block, candidates, written);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_stmt(init, candidates, written);
            }
            collect_block(body, candidates, written);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_block(body, candidates, written);
        }
        Stmt::Block(block) => collect_block(block, candidates, written),
        Stmt::Decl { init, .. } => {
            if let Some(init) = init {
                collect_expr(init, candidates, written);
            }
        }
        Stmt::Return(value) => {
            if let Some(value) = value {
                collect_expr(value, candidates, written);
            }
        }
        Stmt::Jump | Stmt::Opaque => {}
    }
}

/// Collect the varyings an expression writes to.
///
/// Only the increment and the decrement write: every other operation only
/// reads its operands.
fn collect_expr(expr: &Expr, candidates: &Set<String>, written: &mut Vec<String>) {
    match expr {
        Expr::Unary { op, operand, .. } => {
            if let UnOp::IncDec = op
                && let Some(name) = variable_of(operand)
            {
                record(name, candidates, written);
            }
            collect_expr(operand, candidates, written);
        }
        Expr::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, candidates, written);
            collect_expr(rhs, candidates, written);
        }
        Expr::Ternary {
            cond,
            then_expr,
            other,
            ..
        } => {
            collect_expr(cond, candidates, written);
            collect_expr(then_expr, candidates, written);
            collect_expr(other, candidates, written);
        }
        Expr::Paren { inner, .. } => collect_expr(inner, candidates, written),
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr(arg, candidates, written);
            }
        }
        Expr::Swizzle { base, .. } => collect_expr(base, candidates, written),
        Expr::Index { base, index, .. } => {
            collect_expr(base, candidates, written);
            collect_expr(index, candidates, written);
        }
        Expr::Literal { .. } | Expr::Ident { .. } | Expr::Error { .. } => {}
    }
}

/// The name of the variable an expression refers to.
///
/// Only the expressions that name a variable have a name: a literal, a call
/// or an operation does not.
fn variable_of(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident { name, .. } => Some(name),
        Expr::Swizzle { base, .. } => variable_of(base),
        Expr::Index { base, .. } => variable_of(base),
        Expr::Paren { inner, .. } => variable_of(inner),
        _ => None,
    }
}

/// Remember that a varying is written to.
fn record(name: &str, candidates: &Set<String>, written: &mut Vec<String>) {
    if !candidates.contains(name) {
        return;
    }
    if written.iter().any(|written| written == name) {
        return;
    }
    written.push(name.to_string());
}

/// Replace the writes to the varyings of a fragment shader.
///
/// The declaration of a varying that is written to is renamed: the shader
/// reads the input under another name and writes to a local variable
/// instead.  Vulkan GLSL does not allow the fragment shader to write to an
/// input, so the write would be rejected otherwise.
pub fn fix_fragment_writes(source: &str, layout: &EffectLayout) -> String {
    let candidates: Set<String> = layout.varying_locations.keys().cloned().collect();
    let written = written_varyings(source, &candidates);
    if written.is_empty() {
        return source.to_string();
    }

    let mut result = source.to_string();
    for name in &written {
        let input_name = format!("_in_{name}");
        let Some((type_name, size)) = rename_declaration(&mut result, name, &input_name) else {
            // The varying is not declared by the shader: there is nothing
            // to rename.
            continue;
        };
        insert_at_main(
            &mut result,
            &format!("{type_name} {name}{size} = {input_name};"),
        );
    }
    result
}

/// The declaration of a varying, with the name of the varying replaced.
///
/// The declaration of a varying of the fragment shader reads
/// `layout(location=N) in TYPE NAME;`.  The type of the varying and the
/// size of its array are returned, so that a local copy of the varying has
/// the type of the varying itself.
fn rename_declaration(
    source: &mut String,
    name: &str,
    new_name: &str,
) -> Option<(String, String)> {
    let mut result = String::with_capacity(source.len());
    let mut declaration: Option<(String, String)> = None;

    for line in source.lines() {
        if declaration.is_none()
            && let Some(found) = declaration_of(line, name)
        {
            // The name of the declaration is replaced, the rest of the
            // line stays as it is.
            let renamed = replace_word(line, name, new_name);
            result.push_str(&renamed);
            result.push('\n');
            declaration = Some(found);
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if declaration.is_some() {
        *source = result;
    }
    declaration
}

/// The type and the size of the array of the declaration of a varying.
///
/// The declaration of a varying of the fragment shader reads
/// `layout(location=N) in TYPE NAME;`, the size of the array is behind the
/// name (`TYPE NAME[4];`).
fn declaration_of(line: &str, name: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    // Only a line that declares an input can declare a varying.
    let rest = if trimmed.starts_with("layout(") {
        let index = trimmed.find(") in ")? + ") in ".len();
        &trimmed[index..]
    } else {
        trimmed.strip_prefix("in ")?
    };
    let rest = rest.trim();
    let (type_name, declarator) = rest.split_once(char::is_whitespace)?;
    let declarator = declarator.trim();
    let declared = declarator.trim_end_matches(';');
    let (declared, size) = match declared.split_once('[') {
        Some((declared, size)) => {
            let size = format!("[{}]", size.trim_end_matches(';'));
            (declared, size)
        }
        None => (declared, String::new()),
    };
    if declared != name {
        return None;
    }
    Some((type_name.to_string(), size))
}

/// Replace the occurrences of a name that are a whole word.
///
/// The name `v_UV` is not replaced in `my_v_UV`: the two are different
/// variables.
fn replace_word(text: &str, name: &str, new_name: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(name) {
        let before = index == 0 || !is_identifier_char(rest.as_bytes()[index - 1]);
        let after = index + name.len();
        let behind = after >= rest.len() || !is_identifier_char(rest.as_bytes()[after]);
        result.push_str(&rest[..index]);
        if before && behind {
            result.push_str(new_name);
        } else {
            result.push_str(name);
        }
        rest = &rest[after..];
    }
    result.push_str(rest);
    result
}

/// Whether a byte is a part of an identifier.
fn is_identifier_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Insert a statement after the opening brace of `main`.
pub(super) fn insert_at_main(output: &mut String, stmt: &str) {
    if let Some(main_pos) = output.find("void main()")
        && let Some(brace_pos) = output[main_pos..].find('{')
    {
        output.insert_str(main_pos + brace_pos + 1, &format!("\n    {}", stmt));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::new_set;

    /// The candidates of a test: the varyings of a shader.
    fn candidates(names: &[&str]) -> Set<String> {
        let mut set = new_set();
        for name in names {
            set.insert(name.to_string());
        }
        set
    }

    /// A write to a varying is found.
    #[test]
    fn a_write_to_a_varying_is_found() {
        let source = "\
layout(location=0) in vec2 v_UV;

void main() {
    v_UV = vec2(1.0);
}
";
        let written = written_varyings(source, &candidates(&["v_UV"]));
        assert_eq!(written, ["v_UV"]);
    }

    /// A write without spaces around the `=` is found.
    #[test]
    fn a_write_without_spaces_is_found() {
        let source = "\
layout(location=0) in vec2 v_UV;

void main() {
    v_UV=vec2(1.0);
}
";
        let written = written_varyings(source, &candidates(&["v_UV"]));
        assert_eq!(written, ["v_UV"]);
    }

    /// A write to a part of a varying is found.
    #[test]
    fn a_write_to_a_part_of_a_varying_is_found() {
        let source = "\
layout(location=0) in vec4 v_UV;

void main() {
    v_UV.xy = vec2(1.0);
}
";
        let written = written_varyings(source, &candidates(&["v_UV"]));
        assert_eq!(written, ["v_UV"]);
    }

    /// The name of a varying that is the tail of the name of another one
    /// does not trigger a write.
    #[test]
    fn the_name_of_another_varying_is_not_a_write() {
        let source = "\
layout(location=0) in vec2 v_UV;
layout(location=1) in vec2 my_v_UV;

void main() {
    my_v_UV = vec2(1.0);
}
";
        let written = written_varyings(source, &candidates(&["v_UV", "my_v_UV"]));
        assert_eq!(written, ["my_v_UV"]);
    }

    /// A write in a branch of a conditional is found.
    #[test]
    fn a_write_in_a_conditional_is_found() {
        let source = "\
layout(location=0) in vec2 v_UV;

void main() {
    if (true) {
        v_UV = vec2(1.0);
    } else {
        v_UV.xy = vec2(0.0);
    }
}
";
        let written = written_varyings(source, &candidates(&["v_UV"]));
        assert_eq!(written, ["v_UV"]);
    }

    /// A shader that only reads its varyings writes to none of them.
    #[test]
    fn a_shader_that_only_reads_writes_to_nothing() {
        let source = "\
layout(location=0) in vec2 v_UV;

void main() {
    vec2 value = v_UV * 2.0;
    _fragColor = vec4(value, 0.0, 1.0);
}
";
        let written = written_varyings(source, &candidates(&["v_UV"]));
        assert!(written.is_empty());
    }

    /// The word of a name is replaced, the tail of another name is not.
    #[test]
    fn a_word_is_replaced_and_a_tail_is_not() {
        assert_eq!(
            replace_word("layout(location=0) in vec2 v_UV;", "v_UV", "_in_v_UV"),
            "layout(location=0) in vec2 _in_v_UV;"
        );
        assert_eq!(
            replace_word("my_v_UV = v_UV;", "v_UV", "_in_v_UV"),
            "my_v_UV = _in_v_UV;"
        );
    }
}
