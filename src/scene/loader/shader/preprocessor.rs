//! The GLSL preprocessor of the shader loader.
//!
//! Wallpaper Engine ships its shaders with the conditionals of a C
//! preprocessor, and the conditions are not trivial: the shaders use
//! `A == 1 && B == 2`, `(LIGHTING || REFLECTION) && EMISSIVE_MAP` and
//! `TEX1FORMAT >= FORMAT_ETC1_RGB8 && TEX1FORMAT <= FORMAT_DXT1 ||
//! TEX1FORMAT == FORMAT_BC7`.  The conditions are resolved here, with the
//! expression evaluator of [`super::c_expr`], and the result is a flat GLSL
//! source that the shader compiler can read.
//!
//! The preprocessor implements the semantics of the C preprocessor:
//!
//! * The conditionals - `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else` and
//!   `#endif` - are nested and resolved here.  A missing `#endif` is a
//!   warning, not a silent end of the file.
//! * A `#define` of a branch that is taken exists from that point on, in the
//!   file that defines it and in every file it includes afterwards.  A
//!   `#define` of a branch that is not taken does not exist at all.
//! * The macros of a header become visible to the file that includes it, and
//!   only the headers that are really included contribute their macros.
//! * An `#include` that cannot be resolved is a warning: the shader would
//!   miss the functions of the header otherwise.
//!
//! The macros that come from outside the source - the values of the combos
//! of the material - win over a `#define` of the source.  The definition of
//! the source is dropped in that case, so that the shader compiler does not
//! see the macro twice.

use crate::collections::{Map, new_map};

use super::c_expr::{self, Macro, MacroMap};

/// The macros the preprocessor defines itself.
///
/// `GLSL` tells the shaders that they are compiled as GLSL, and the five
/// constants are the ones the Wallpaper Engine headers define.  They are
/// defined here as well, so that a shader that uses them compiles without
/// the Wallpaper Engine assets.
pub const BUILTIN_DEFINES: &[(&str, &str)] = &[
    ("GLSL", "1"),
    ("M_PI", "3.14159265358979323846"),
    ("M_PI_HALF", "1.57079632679489661923"),
    ("M_PI_2", "6.28318530717958647692"),
    ("SQRT_2", "1.41421356237309504880"),
    ("SQRT_3", "1.73205080756887729353"),
];

/// A line of a flattened source.
#[derive(Debug, Clone)]
pub struct Line {
    /// The text of the line.
    pub text: String,
    /// Whether the line comes from an included header.
    pub from_header: bool,
}

/// The result of the preprocessing of a source.
#[derive(Debug, Default)]
pub struct Flattened {
    /// The lines that survive the preprocessing.
    pub lines: Vec<Line>,
    /// The warnings the preprocessing produced.
    pub warnings: Vec<String>,
}

/// Flatten a source into the GLSL that is handed to the shader compiler.
///
/// The conditionals are resolved, the headers that are included are inlined
/// and the macros of the source and of its headers are collected.  `defines`
/// holds the macros that come from outside the source - the values of the
/// combos of the material - and they win over a `#define` of the source.
pub fn flatten(
    source: &str,
    headers: &Map<String, String>,
    defines: &Map<String, String>,
) -> Flattened {
    let external = c_expr::macro_map_from_values(defines);

    let mut context = Context::new(headers, external);
    context.process(source, false);

    // The source ends with an open conditional: the `#endif` is missing.
    if !context.stack.is_empty() {
        context.warn("the source ends with a `#if` that is never closed".to_string());
        context.stack.clear();
    }

    // The macros of the preprocessor itself come first, so that a `#define`
    // of the source cannot redefine them.
    let mut lines = Vec::with_capacity(context.lines.len() + BUILTIN_DEFINES.len());
    for (name, value) in BUILTIN_DEFINES {
        lines.push(Line {
            text: format!("#define {} {}", name, value),
            from_header: false,
        });
    }
    lines.extend(context.lines);

    Flattened {
        lines,
        warnings: context.warnings,
    }
}

/// Collect the macros a source defines.
///
/// Only the macros of the branches that are taken are collected: a macro
/// that is defined in a branch that is not taken does not exist.  The
/// branches are evaluated with the macros that were defined before them, so
/// a macro that depends on a combo of the material is not collected here -
/// the preprocessor defines it when the shader is compiled.
pub fn collect_defines(source: &str) -> MacroMap {
    let headers = new_map();
    let mut context = Context::new(&headers, new_map());
    // The headers are not inlined: the macros of a header belong to the
    // header, not to the file that includes it.
    context.follow_includes = false;
    context.process(source, false);
    context.own
}

/// The state of a conditional block.
struct CondFrame {
    /// Whether the condition of the current branch was true.
    taken: bool,
    /// Whether the lines of the current branch are part of the output.
    active: bool,
    /// Whether an `#else` was seen for this chain.
    seen_else: bool,
}

/// The state of the preprocessing.
struct Context<'a> {
    /// The headers an `#include` can resolve to.
    headers: &'a Map<String, String>,
    /// The macros that come from outside the source.  They win over a
    /// `#define` of the source.
    external: MacroMap,
    /// The macros that are currently defined.
    defines: MacroMap,
    /// The macros the source itself defines.
    own: MacroMap,
    /// The stack of the open conditional blocks.
    stack: Vec<CondFrame>,
    /// The lines of the flattened source.
    lines: Vec<Line>,
    /// The warnings of the preprocessing.
    warnings: Vec<String>,
    /// The headers that are being included right now.
    including: Vec<String>,
    /// Whether the `#include` directives are followed.
    follow_includes: bool,
}

impl<'a> Context<'a> {
    /// A context for a source with the given headers and external macros.
    fn new(headers: &'a Map<String, String>, external: MacroMap) -> Self {
        let mut defines = external.clone();
        for (name, value) in BUILTIN_DEFINES {
            defines
                .entry(name.to_string())
                .or_insert_with(|| Macro::object(value.to_string()));
        }

        Self {
            headers,
            external,
            defines,
            own: new_map(),
            stack: Vec::new(),
            lines: Vec::new(),
            warnings: Vec::new(),
            including: Vec::new(),
            follow_includes: true,
        }
    }

    /// Whether the lines of the current branch are part of the output.
    fn active(&self) -> bool {
        self.stack.last().map(|frame| frame.active).unwrap_or(true)
    }

    /// Whether the branch that contains the current conditional is active.
    ///
    /// This is the state of the stack without its last frame: the frame of
    /// the conditional itself does not count.
    fn enclosing_active(&self) -> bool {
        match self.stack.len() {
            0 | 1 => true,
            length => self.stack[length - 2].active,
        }
    }

    /// Record a warning.
    fn warn(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Process the lines of a source.
    fn process(&mut self, source: &str, from_header: bool) {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                self.directive(trimmed, from_header);
                continue;
            }
            if !self.active() {
                // The line is dropped, but the line count is kept so that
                // the line numbers of the shader stay close to the original.
                self.lines.push(Line {
                    text: String::new(),
                    from_header,
                });
                continue;
            }
            self.lines.push(Line {
                text: line.to_string(),
                from_header,
            });
        }
    }

    /// Process a preprocessor directive.
    fn directive(&mut self, trimmed: &str, from_header: bool) {
        let (name, rest) = split_directive(trimmed);

        match name {
            "if" => self.if_directive(rest),
            "ifdef" => self.ifdef_directive(rest, true),
            "ifndef" => self.ifdef_directive(rest, false),
            "elif" => self.elif_directive(rest),
            "else" => self.else_directive(),
            "endif" => self.endif_directive(),
            "define" => self.define_directive(rest, trimmed, from_header),
            _ => {
                // Everything else is only read in a branch that is taken.
                if !self.active() {
                    return;
                }
                match name {
                    "undef" => self.undef_directive(rest, trimmed, from_header),
                    "include" => self.include_directive(rest),
                    // The version of the shader is the one the pipeline emits.
                    "version" => {}
                    // The directives the shader compiler knows.
                    "extension" | "pragma" | "line" => self.lines.push(Line {
                        text: trimmed.to_string(),
                        from_header,
                    }),
                    // An unknown directive - `#require` of the engine - is
                    // dropped: the shader compiler would not understand it
                    // either.
                    _ => {
                        self.warn(format!(
                            "the directive `{}` is not known and is ignored",
                            trimmed.trim()
                        ));
                    }
                }
            }
        }
    }

    /// Process an `#if`.
    fn if_directive(&mut self, rest: &str) {
        let enclosing = self.active();
        let condition = if enclosing {
            self.evaluate(rest)
        } else {
            false
        };
        self.stack.push(CondFrame {
            taken: condition,
            active: condition && enclosing,
            seen_else: false,
        });
    }

    /// Process an `#ifdef` or an `#ifndef`.
    fn ifdef_directive(&mut self, rest: &str, expected: bool) {
        let name = strip_comment(rest);
        let enclosing = self.active();
        let condition = self.defines.contains_key(name) == expected;
        self.stack.push(CondFrame {
            taken: condition,
            active: condition && enclosing,
            seen_else: false,
        });
    }

    /// Process an `#elif`.
    fn elif_directive(&mut self, rest: &str) {
        let Some((taken, seen_else)) = self
            .stack
            .last()
            .map(|frame| (frame.taken, frame.seen_else))
        else {
            self.warn("an `#elif` without an `#if`".to_string());
            return;
        };
        if seen_else {
            self.warn("an `#elif` after an `#else`".to_string());
        }

        let enclosing = self.enclosing_active();
        if taken {
            // A branch of this chain was already taken.
            if let Some(frame) = self.stack.last_mut() {
                frame.active = false;
            }
            return;
        }

        let condition = if enclosing {
            self.evaluate(rest)
        } else {
            false
        };
        if let Some(frame) = self.stack.last_mut() {
            frame.taken = condition;
            frame.active = condition && enclosing;
        }
    }

    /// Process an `#else`.
    fn else_directive(&mut self) {
        let Some((taken, seen_else)) = self
            .stack
            .last()
            .map(|frame| (frame.taken, frame.seen_else))
        else {
            self.warn("an `#else` without an `#if`".to_string());
            return;
        };
        if seen_else {
            self.warn("a second `#else`".to_string());
        }

        let enclosing = self.enclosing_active();
        if let Some(frame) = self.stack.last_mut() {
            frame.seen_else = true;
            frame.active = !taken && enclosing;
            frame.taken = true;
        }
    }

    /// Process an `#endif`.
    fn endif_directive(&mut self) {
        if self.stack.pop().is_none() {
            self.warn("an `#endif` without an `#if`".to_string());
        }
    }

    /// Process a `#define`.
    fn define_directive(&mut self, rest: &str, original: &str, from_header: bool) {
        let Some(definition) = parse_define(rest) else {
            self.warn(format!("`#define` without a name: {}", original.trim()));
            return;
        };
        let name = definition.name;

        if !self.active() {
            // A macro that is defined in a branch that is not taken does not
            // exist.  It may still be known from the outside - the macros of
            // the source are collected before it is compiled - so it is
            // removed from the macro table.
            if let Some(external) = self.external.get(&name)
                && external.body == definition.value
            {
                self.external.remove(&name);
                self.defines.remove(&name);
                self.own.remove(&name);
            }
            return;
        }

        if self.is_overridden(&name, &definition.value) {
            // The value from outside the source wins: the definition of the
            // source is dropped, so that the shader compiler does not see
            // the macro twice.
            return;
        }

        let definition = Macro {
            body: definition.value,
            params: definition.params,
            function_like: definition.function_like,
        };
        self.defines.insert(name.clone(), definition.clone());
        self.own.insert(name, definition);
        self.lines.push(Line {
            text: original.trim().to_string(),
            from_header,
        });
    }

    /// Process an `#undef`.
    fn undef_directive(&mut self, rest: &str, original: &str, from_header: bool) {
        let name = strip_comment(rest);
        if name.is_empty() {
            self.warn(format!("`#undef` without a name: {}", original.trim()));
            return;
        }
        self.defines.remove(name);
        self.external.remove(name);
        self.own.remove(name);
        // The line is kept, so that the shader compiler sees the same macros.
        self.lines.push(Line {
            text: original.trim().to_string(),
            from_header,
        });
    }

    /// Process an `#include`.
    fn include_directive(&mut self, rest: &str) {
        if !self.follow_includes {
            return;
        }

        let Some(name) = parse_include(rest) else {
            self.warn(format!("`#include` without a file name: {}", rest.trim()));
            return;
        };

        let Some(content) = self.resolve(&name).cloned() else {
            self.warn(format!("the header `{}` cannot be resolved", name));
            return;
        };

        if self.including.iter().any(|included| included == &name) {
            self.warn(format!("the header `{}` includes itself", name));
            return;
        }

        self.including.push(name.clone());
        let depth = self.stack.len();
        self.process(&content, true);
        if self.stack.len() > depth {
            self.warn(format!(
                "the header `{}` has a `#if` that is never closed",
                name
            ));
            self.stack.truncate(depth);
        }
        self.including.pop();
    }

    /// Whether a macro of the source is overridden by a macro from outside.
    fn is_overridden(&self, name: &str, value: &str) -> bool {
        if BUILTIN_DEFINES.iter().any(|(builtin, _)| *builtin == name) {
            return true;
        }
        match self.external.get(name) {
            Some(definition) => definition.body != value,
            None => false,
        }
    }

    /// Find the header of an `#include`.
    ///
    /// A header can be named with or without a path: `common.h` and
    /// `base/common.h` are the same header.
    fn resolve(&self, name: &str) -> Option<&String> {
        if let Some(content) = self.headers.get(name) {
            return Some(content);
        }
        let base = name.rsplit('/').next().unwrap_or(name);
        if base != name {
            return self.headers.get(base);
        }
        None
    }

    /// Evaluate the condition of a directive.
    fn evaluate(&mut self, condition: &str) -> bool {
        let condition = strip_comment(condition);
        match c_expr::evaluate(condition, &self.defines) {
            Ok(value) => value,
            Err(error) => {
                self.warn(format!(
                    "the condition `{}` cannot be evaluated: {}",
                    condition.trim(),
                    error
                ));
                false
            }
        }
    }
}

/// Split a directive into its name and its argument.
fn split_directive(line: &str) -> (&str, &str) {
    let text = line.strip_prefix('#').unwrap_or(line).trim_start();
    let end = text
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim())
}

/// Remove the comment of a directive argument.
fn strip_comment(text: &str) -> &str {
    match text.find("//") {
        Some(position) => text[..position].trim_end(),
        None => text.trim(),
    }
}

/// A `#define` directive.
struct Define {
    /// The name of the macro.
    name: String,
    /// The value of the macro.
    value: String,
    /// The parameters of a function-like macro.
    params: Vec<String>,
    /// Whether the macro is function-like.
    function_like: bool,
}

/// Parse a `#define`.
///
/// The result is the name of the macro, its value, the parameters of a
/// function-like macro and whether it is function-like at all.
fn parse_define(text: &str) -> Option<Define> {
    let text = strip_comment(text);
    if text.is_empty() {
        return None;
    }

    let mut end = text.len();
    for (index, character) in text.char_indices() {
        if character == '_' || character.is_ascii_alphanumeric() {
            continue;
        }
        end = index;
        break;
    }
    let name = text[..end].to_string();
    if name.is_empty() {
        return None;
    }

    let mut rest = text[end..].trim_start();
    let mut params = Vec::new();
    let mut function_like = false;

    if rest.starts_with('(') {
        let close = rest.find(')')?;
        params = rest[1..close]
            .split(',')
            .map(|param| param.trim().to_string())
            .filter(|param| !param.is_empty())
            .collect();
        function_like = true;
        rest = rest[close + 1..].trim_start();
    }

    Some(Define {
        name,
        value: rest.to_string(),
        params,
        function_like,
    })
}

/// Parse the file name of an `#include`.
fn parse_include(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(start) = text.find('"') {
        let rest = &text[start + 1..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    if let Some(start) = text.find('<') {
        let rest = &text[start + 1..];
        let end = rest.find('>')?;
        return Some(rest[..end].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The macros of a test, as the plain map the pipeline uses.
    fn defines(entries: &[(&str, &str)]) -> Map<String, String> {
        let mut map = new_map();
        for (name, value) in entries {
            map.insert(name.to_string(), value.to_string());
        }
        map
    }

    /// Flatten a source and return the lines that survive.
    fn flatten_source(
        source: &str,
        headers: &Map<String, String>,
        macros: &[(&str, &str)],
    ) -> Vec<String> {
        flatten(source, headers, &defines(macros))
            .lines
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    #[test]
    fn a_conditional_is_resolved() {
        let source = "#if FOO\nthe foo branch\n#else\nthe other branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[("FOO", "1")]);
        assert!(lines.iter().any(|line| line.contains("the foo branch")));
        assert!(!lines.iter().any(|line| line.contains("the other branch")));

        let lines = flatten_source(source, &new_map(), &[]);
        assert!(!lines.iter().any(|line| line.contains("the foo branch")));
        assert!(lines.iter().any(|line| line.contains("the other branch")));
    }

    #[test]
    fn an_elif_chain_is_resolved() {
        let source = "#if QUALITY == 4\nfour\n#elif QUALITY == 3\nthree\n#elif QUALITY == 2\ntwo\n#else\nother\n#endif\n";
        for (quality, expected) in [("4", "four"), ("3", "three"), ("2", "two"), ("1", "other")] {
            let lines = flatten_source(source, &new_map(), &[("QUALITY", quality)]);
            let taken: Vec<&String> = lines
                .iter()
                .filter(|line| ["four", "three", "two", "other"].contains(&line.as_str()))
                .collect();
            assert_eq!(taken.len(), 1, "the quality {quality}");
            assert_eq!(taken[0], expected, "the quality {quality}");
        }
    }

    #[test]
    fn the_conditionals_are_nested() {
        let source = "#if OUTER\n#if INNER\nboth\n#else\nonly the outer\n#endif\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[("OUTER", "1"), ("INNER", "1")]);
        assert!(lines.iter().any(|line| line.contains("both")));
        assert!(!lines.iter().any(|line| line.contains("only the outer")));

        let lines = flatten_source(source, &new_map(), &[("OUTER", "1")]);
        assert!(!lines.iter().any(|line| line.contains("both")));
        assert!(lines.iter().any(|line| line.contains("only the outer")));

        let lines = flatten_source(source, &new_map(), &[("INNER", "1")]);
        assert!(!lines.iter().any(|line| line.contains("both")));
        assert!(!lines.iter().any(|line| line.contains("only the outer")));
    }

    #[test]
    fn a_missing_endif_is_a_warning() {
        let mut headers = new_map();
        headers.insert("broken.h".to_string(), "#if FOO\nthe branch\n".to_string());

        let flattened = flatten("#include \"broken.h\"\n", &headers, &new_map());
        assert!(
            flattened
                .warnings
                .iter()
                .any(|warning| warning.contains("never closed")),
            "the warnings are: {:?}",
            flattened.warnings
        );
    }

    #[test]
    fn an_unmatched_endif_is_a_warning() {
        let flattened = flatten("#endif\n", &new_map(), &new_map());
        assert!(
            flattened
                .warnings
                .iter()
                .any(|warning| warning.contains("`#endif`")),
            "the warnings are: {:?}",
            flattened.warnings
        );
    }

    #[test]
    fn a_define_of_an_active_branch_exists_afterwards() {
        let source = "#if FOO\n#define MYCONST 1\n#endif\n#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[("FOO", "1")]);
        assert!(lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn a_define_of_an_inactive_branch_does_not_exist() {
        let source = "#if 0\n#define MYCONST 1\n#endif\n#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[]);
        assert!(!lines.iter().any(|line| line.contains("the branch")));
        assert!(!lines.iter().any(|line| line.contains("#define MYCONST")));
    }

    #[test]
    fn an_undef_removes_a_macro() {
        let source = "#define MYCONST 1\n#undef MYCONST\n#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[]);
        assert!(!lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn the_macros_of_a_header_are_visible_to_the_file_that_includes_it() {
        let mut headers = new_map();
        headers.insert("common.h".to_string(), "#define MYCONST 1\n".to_string());
        let source = "#include \"common.h\"\n#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &headers, &[]);
        assert!(lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn the_macros_of_a_header_are_not_visible_without_the_include() {
        let mut headers = new_map();
        headers.insert("common.h".to_string(), "#define MYCONST 1\n".to_string());
        let source = "#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &headers, &[]);
        assert!(!lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn only_the_included_headers_contribute_their_macros() {
        let mut headers = new_map();
        headers.insert("common.h".to_string(), "#define MYCONST 1\n".to_string());
        headers.insert("other.h".to_string(), "#define OTHERCONST 1\n".to_string());
        let source = "#include \"common.h\"\n#ifdef OTHERCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &headers, &[]);
        assert!(!lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn a_header_can_be_included_with_a_path() {
        let mut headers = new_map();
        headers.insert(
            "model_vertex_v1.h".to_string(),
            "#define MYCONST 1\n".to_string(),
        );
        let source = "#include \"base/model_vertex_v1.h\"\n#ifdef MYCONST\nthe branch\n#endif\n";
        let lines = flatten_source(source, &headers, &[]);
        assert!(lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn an_include_that_cannot_be_resolved_is_a_warning() {
        let flattened = flatten("#include \"missing.h\"\n", &new_map(), &new_map());
        assert!(
            flattened
                .warnings
                .iter()
                .any(|warning| warning.contains("missing.h")),
            "the warnings are: {:?}",
            flattened.warnings
        );
    }

    #[test]
    fn a_macro_from_outside_wins_over_the_definition_of_the_source() {
        let source = "#define BLENDMODE 0\n#ifdef BLENDMODE\nthe branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[("BLENDMODE", "26")]);
        // The definition of the source is dropped, so that the shader
        // compiler does not see the macro twice.
        assert!(!lines.iter().any(|line| line.contains("#define BLENDMODE")));
        assert!(lines.iter().any(|line| line.contains("the branch")));
    }

    #[test]
    fn the_mathematical_constants_are_always_defined() {
        let lines = flatten_source("void main() {}\n", &new_map(), &[]);
        for (name, _) in BUILTIN_DEFINES {
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with(&format!("#define {name} "))),
                "the constant {name} is missing"
            );
        }
    }

    #[test]
    fn glsl_is_defined_and_hlsl_is_not() {
        let source = "#ifdef GLSL\nthe glsl branch\n#endif\n#ifdef HLSL\nthe hlsl branch\n#endif\n";
        let lines = flatten_source(source, &new_map(), &[]);
        assert!(lines.iter().any(|line| line.contains("the glsl branch")));
        assert!(!lines.iter().any(|line| line.contains("the hlsl branch")));
    }

    #[test]
    fn an_unknown_directive_is_dropped() {
        let flattened = flatten(
            "#require LightingV1\nvoid main() {}\n",
            &new_map(),
            &new_map(),
        );
        assert!(
            !flattened
                .lines
                .iter()
                .any(|line| line.text.starts_with("#require")),
            "the directive survived"
        );
        assert!(
            flattened
                .warnings
                .iter()
                .any(|warning| warning.contains("#require")),
            "the warnings are: {:?}",
            flattened.warnings
        );
    }

    #[test]
    fn the_matrices_are_collected() {
        let mut macros = new_map();
        macros.insert("FOO".to_string(), Macro::object("1".to_string()));
        let collected = collect_defines("#define A 1\n#if FOO\n#define B 2\n#endif\n");
        assert_eq!(
            collected.get("A").map(|macro_| macro_.body.as_str()),
            Some("1")
        );
        assert!(collected.get("B").is_none());
    }
}
