//! Code profile (D89): modules, top-level symbols, imports and dependencies from a syntax tree.
//!
//! The text parser keeps the file as paragraph blocks with exact line and character anchors. The source is rebuilt from
//! them (the indentation of a paragraph's first line is recovered from the gap between anchors), parsed with the
//! tree-sitter grammar of the language, and the top level of the tree is read: functions, classes, types (and the
//! methods of the classes and impls), imports. Nothing is guessed by a model, and nothing is extracted per line: a file
//! yields at most [`MAX_SYMBOLS`] symbols and [`MAX_IMPORTS`] dependencies, a file of more than [`MAX_SOURCE_BYTES`] is
//! indexed but not parsed. A symbol is named `path::name` so two files never share one by accident; a dependency is
//! named by its package, so every file that imports it shares the entity.

use oo_core::docir::{Anchor, DocIr};
use tree_sitter::{Language, Node, Parser};

use super::{CLASS, DEPENDENCY, FUNCTION, PATH, TYPE, clip, code_language, is_manifest, thing};
use crate::extract::model::{Collector, EntityRef, EvidenceRef};

pub const MAX_SYMBOLS: usize = 40;
pub const MAX_IMPORTS: usize = 40;
pub const MAX_SOURCE_BYTES: usize = 1_500_000;
const SYMBOL_BP: u16 = 9500;

/// The source text of a parsed text file, with every line at its own line number.
/// (first line, last line, block index) of a block.
pub type BlockLines = (u64, u64, usize);

pub fn rebuild_source(ir: &DocIr) -> Option<(String, Vec<BlockLines>)> {
    // (line_start, line_end, block index) of the blocks that carry a text anchor, in file order.
    let mut spans: Vec<(u64, u64, u64, u64, usize)> = Vec::new();
    for (i, b) in ir.blocks.iter().enumerate() {
        if let Anchor::Text {
            line_start,
            line_end,
            char_start,
            char_end,
        } = &b.anchor
        {
            spans.push((
                u64::from(*line_start),
                u64::from(*line_end),
                *char_start,
                *char_end,
                i,
            ));
        }
    }
    if spans.is_empty() || spans.len() != ir.blocks.len() {
        return None;
    }
    spans.sort_by_key(|s| (s.0, s.2));
    // Oversize paragraphs were split into chunks that share an anchor: the source is not recoverable.
    if spans
        .windows(2)
        .any(|w| w[0].0 == w[1].0 && w[0].2 == w[1].2)
    {
        return None;
    }
    let crlf = spans.iter().any(|&(ls, le, cs, ce, i)| {
        le > ls && (ce - cs) as usize == ir.blocks[i].text.chars().count() + (le - ls) as usize
    });
    let nl = if crlf { 2 } else { 1 };
    let mut lines: Vec<String> = Vec::new();
    let mut prev_line_end: u64 = 0;
    let mut prev_char_end: u64 = 0;
    let mut index: Vec<(u64, u64, usize)> = Vec::new();
    for (ls, le, cs, ce, i) in spans {
        if ls == 0 || le < ls || le > 400_000 {
            return None;
        }
        let expected_nl = if prev_line_end == 0 {
            ls - 1
        } else {
            ls.saturating_sub(prev_line_end)
        };
        let gap = cs.saturating_sub(prev_char_end);
        let extra = (gap as usize).saturating_sub(expected_nl as usize * nl);
        let text = &ir.blocks[i].text;
        let tab = text.lines().skip(1).any(|l| l.starts_with('\t'));
        let indent: String =
            std::iter::repeat_n(if tab { '\t' } else { ' ' }, extra.min(200)).collect();
        while (lines.len() as u64) < ls - 1 {
            lines.push(String::new());
        }
        for (k, l) in text.split('\n').enumerate() {
            let n = (ls - 1) as usize + k;
            let line = if k == 0 {
                format!("{indent}{l}")
            } else {
                l.to_owned()
            };
            if n < lines.len() {
                lines[n] = line;
            } else {
                lines.push(line);
            }
        }
        index.push((ls, le, i));
        prev_line_end = le;
        prev_char_end = ce;
    }
    let mut src = lines.join("\n");
    src.push('\n');
    Some((src, index))
}

fn language_of(lang: &str) -> Option<Language> {
    Some(match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        _ => return None,
    })
}

/// One symbol read from the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub label: &'static str,
    pub name: String,
    /// 1-based line.
    pub line: u64,
}

/// What the tree says about a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    /// Imported packages (external only), with the line of the first import.
    pub imports: Vec<(String, u64)>,
}

fn text<'a>(n: Node<'_>, src: &'a str) -> &'a str {
    src.get(n.byte_range()).unwrap_or("")
}

fn field_text<'a>(n: Node<'_>, field: &str, src: &'a str) -> Option<&'a str> {
    n.child_by_field_name(field)
        .map(|c| text(c, src))
        .filter(|t| !t.is_empty())
}

fn line(n: Node<'_>) -> u64 {
    n.start_position().row as u64 + 1
}

const PY_STDLIB: &[&str] = &[
    "os",
    "sys",
    "re",
    "json",
    "math",
    "time",
    "datetime",
    "typing",
    "collections",
    "itertools",
    "functools",
    "pathlib",
    "logging",
    "argparse",
    "subprocess",
    "threading",
    "asyncio",
    "dataclasses",
    "enum",
    "abc",
    "io",
    "copy",
    "random",
    "string",
    "hashlib",
    "base64",
    "uuid",
    "tempfile",
    "shutil",
    "glob",
    "csv",
    "sqlite3",
    "unittest",
    "contextlib",
    "textwrap",
    "traceback",
    "warnings",
    "inspect",
    "types",
    "operator",
    "heapq",
    "bisect",
    "struct",
    "socket",
    "http",
    "urllib",
    "email",
    "html",
    "xml",
    "zipfile",
    "tarfile",
    "gzip",
    "pickle",
    "queue",
    "signal",
    "platform",
    "decimal",
    "statistics",
    "__future__",
    "builtins",
];
const RUST_SKIP: &[&str] = &[
    "std",
    "core",
    "alloc",
    "crate",
    "self",
    "super",
    "proc_macro",
];
const NODE_BUILTINS: &[&str] = &[
    "fs",
    "path",
    "os",
    "http",
    "https",
    "url",
    "util",
    "events",
    "stream",
    "crypto",
    "child_process",
    "zlib",
    "net",
    "readline",
    "assert",
    "buffer",
    "process",
    "timers",
    "tty",
    "dns",
    "querystring",
    "string_decoder",
    "worker_threads",
    "module",
];

fn push_symbol(out: &mut Outline, label: &'static str, name: &str, line: u64) {
    if name.is_empty() || out.symbols.len() >= MAX_SYMBOLS {
        return;
    }
    out.symbols.push(Symbol {
        label,
        name: name.to_owned(),
        line,
    });
}

fn push_import(out: &mut Outline, pkg: &str, line: u64) {
    let pkg = pkg.trim();
    if pkg.is_empty()
        || out.imports.len() >= MAX_IMPORTS
        || out.imports.iter().any(|(p, _)| p == pkg)
    {
        return;
    }
    out.imports.push((pkg.to_owned(), line));
}

fn rust_outline(root: Node<'_>, src: &str, out: &mut Outline, prefix: &str, depth: usize) {
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        let l = line(n);
        let qual = |name: &str| {
            if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}::{name}")
            }
        };
        match n.kind() {
            "function_item" | "function_signature_item" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, FUNCTION, &qual(name), l);
                }
            }
            "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, TYPE, &qual(name), l);
                }
                if n.kind() == "trait_item"
                    && let Some(body) = n.child_by_field_name("body")
                {
                    let tname = field_text(n, "name", src).unwrap_or("");
                    rust_outline(body, src, out, &qual(tname), depth + 1);
                }
            }
            "impl_item" => {
                let ty = field_text(n, "type", src).unwrap_or("");
                let ty = ty.split('<').next().unwrap_or(ty).trim();
                if let Some(body) = n.child_by_field_name("body") {
                    rust_outline(body, src, out, &qual(ty), depth + 1);
                }
            }
            "mod_item" if depth < 3 => {
                let name = field_text(n, "name", src).unwrap_or("");
                if name != "tests"
                    && let Some(body) = n.child_by_field_name("body")
                {
                    rust_outline(body, src, out, &qual(name), depth + 1);
                }
            }
            "use_declaration" => {
                if let Some(arg) = n.child_by_field_name("argument") {
                    let t = text(arg, src);
                    let first = t.trim_start_matches("::").split("::").next().unwrap_or("");
                    let first = first.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
                    if !first.is_empty() && !RUST_SKIP.contains(&first) {
                        push_import(out, first, l);
                    }
                }
            }
            "extern_crate_declaration" => {
                if let Some(name) = field_text(n, "name", src)
                    && !RUST_SKIP.contains(&name)
                {
                    push_import(out, name, l);
                }
            }
            _ => {}
        }
    }
}

fn py_outline(root: Node<'_>, src: &str, out: &mut Outline, prefix: &str, depth: usize) {
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        let l = line(n);
        let qual = |name: &str| {
            if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}.{name}")
            }
        };
        let n = if n.kind() == "decorated_definition" {
            n.child_by_field_name("definition").unwrap_or(n)
        } else {
            n
        };
        match n.kind() {
            "function_definition" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, FUNCTION, &qual(name), l);
                }
            }
            "class_definition" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, CLASS, &qual(name), l);
                    if depth < 1
                        && let Some(body) = n.child_by_field_name("body")
                    {
                        py_outline(body, src, out, &qual(name), depth + 1);
                    }
                }
            }
            "import_statement" => {
                let mut c = n.walk();
                for name in n.children_by_field_name("name", &mut c) {
                    let node = if name.kind() == "aliased_import" {
                        name.child_by_field_name("name").unwrap_or(name)
                    } else {
                        name
                    };
                    let top = text(node, src).split('.').next().unwrap_or("");
                    if !PY_STDLIB.contains(&top) {
                        push_import(out, top, l);
                    }
                }
            }
            "import_from_statement" => {
                if let Some(m) = n.child_by_field_name("module_name") {
                    let t = text(m, src);
                    if !t.starts_with('.') {
                        let top = t.split('.').next().unwrap_or("");
                        if !PY_STDLIB.contains(&top) {
                            push_import(out, top, l);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn js_package(spec: &str) -> Option<String> {
    let s = spec.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if s.is_empty()
        || s.starts_with('.')
        || s.starts_with('/')
        || s.starts_with("node:")
        || s.starts_with("~/")
        || s.starts_with("@/")
        || s.starts_with('#')
    {
        return None;
    }
    let mut parts = s.split('/');
    let first = parts.next()?;
    let pkg = if first.starts_with('@') {
        format!("{first}/{}", parts.next()?)
    } else {
        first.to_owned()
    };
    (!NODE_BUILTINS.contains(&pkg.as_str())).then_some(pkg)
}

fn js_outline(root: Node<'_>, src: &str, out: &mut Outline, prefix: &str, depth: usize) {
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        let l = line(n);
        let qual = |name: &str| {
            if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}.{name}")
            }
        };
        let n = if n.kind() == "export_statement" {
            n.child_by_field_name("declaration").unwrap_or(n)
        } else {
            n
        };
        match n.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, FUNCTION, &qual(name), l);
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, CLASS, &qual(name), l);
                    if depth < 1
                        && let Some(body) = n.child_by_field_name("body")
                    {
                        js_outline(body, src, out, &qual(name), depth + 1);
                    }
                }
            }
            "method_definition" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, FUNCTION, &qual(name), l);
                }
            }
            "interface_declaration" | "type_alias_declaration" | "enum_declaration" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, TYPE, &qual(name), l);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                let mut c = n.walk();
                for d in n.children(&mut c) {
                    if d.kind() != "variable_declarator" {
                        continue;
                    }
                    let is_fn = d.child_by_field_name("value").is_some_and(|v| {
                        matches!(
                            v.kind(),
                            "arrow_function"
                                | "function_expression"
                                | "function"
                                | "generator_function"
                        )
                    });
                    if is_fn && let Some(name) = field_text(d, "name", src) {
                        push_symbol(out, FUNCTION, &qual(name), l);
                    }
                }
            }
            "import_statement" => {
                if let Some(s) = n.child_by_field_name("source")
                    && let Some(p) = js_package(text(s, src))
                {
                    push_import(out, &p, l);
                }
            }
            _ => {}
        }
    }
}

fn go_outline(root: Node<'_>, src: &str, out: &mut Outline) {
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        let l = line(n);
        match n.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(n, "name", src) {
                    push_symbol(out, FUNCTION, name, l);
                }
            }
            "method_declaration" => {
                if let Some(name) = field_text(n, "name", src) {
                    let recv = n
                        .child_by_field_name("receiver")
                        .map(|r| {
                            text(r, src)
                                .trim_matches(|c| c == '(' || c == ')')
                                .split_whitespace()
                                .last()
                                .unwrap_or("")
                                .trim_start_matches('*')
                                .to_owned()
                        })
                        .unwrap_or_default();
                    let q = if recv.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{recv}.{name}")
                    };
                    push_symbol(out, FUNCTION, &q, l);
                }
            }
            "type_declaration" => {
                let mut c = n.walk();
                for spec in n.children(&mut c) {
                    if matches!(spec.kind(), "type_spec" | "type_alias")
                        && let Some(name) = field_text(spec, "name", src)
                    {
                        push_symbol(out, TYPE, name, line(spec));
                    }
                }
            }
            "import_declaration" => {
                let mut stack = vec![n];
                while let Some(x) = stack.pop() {
                    let mut c = x.walk();
                    for ch in x.children(&mut c) {
                        if ch.kind() == "import_spec" {
                            if let Some(p) = ch.child_by_field_name("path") {
                                let path = text(p, src).trim_matches('"');
                                let first = path.split('/').next().unwrap_or("");
                                if first.contains('.') {
                                    push_import(out, path, line(ch));
                                }
                            }
                        } else if ch.kind() == "import_spec_list" {
                            stack.push(ch);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Read the symbols and imports of a source text.
pub fn outline(lang: &str, src: &str) -> Option<Outline> {
    let language = language_of(lang)?;
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();
    let mut out = Outline::default();
    match lang {
        "rust" => rust_outline(root, src, &mut out, "", 0),
        "python" => py_outline(root, src, &mut out, "", 0),
        "javascript" | "typescript" | "tsx" => js_outline(root, src, &mut out, "", 0),
        "go" => go_outline(root, src, &mut out),
        _ => return None,
    }
    Some(out)
}

fn symbol_name(path: &str, name: &str) -> String {
    let full = format!("{path}::{name}");
    if full.chars().count() <= 240 {
        return full;
    }
    let tail: String = full
        .chars()
        .rev()
        .take(239)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

pub(crate) fn extract(path: &str, ir: &DocIr, col: &mut Collector) {
    // The package the file sits in: its directory is shared with the instruction documents that name it.
    if let Some((dir, _)) = path.rsplit_once('/')
        && let Some(first) = ir.blocks.first()
    {
        col.entity(&thing(PATH, dir), Some(&EvidenceRef::from_block(first)));
    }
    if is_manifest(path) {
        super::manifest::extract(path, ir, col);
        return;
    }
    let Some(lang) = code_language(path) else {
        return;
    };
    let total: usize = ir.blocks.iter().map(|b| b.text.len()).sum();
    if total > MAX_SOURCE_BYTES {
        return;
    }
    let Some((src, index)) = rebuild_source(ir) else {
        return;
    };
    let Some(out) = outline(lang, &src) else {
        return;
    };
    let block_at = |line: u64| -> Option<&oo_core::docir::Block> {
        index
            .iter()
            .find(|(s, e, _)| *s <= line && line <= *e)
            .or_else(|| index.iter().rev().find(|(s, _, _)| *s <= line))
            .map(|(_, _, i)| &ir.blocks[*i])
    };
    for s in &out.symbols {
        let Some(b) = block_at(s.line) else { continue };
        let e: EntityRef = thing(s.label, symbol_name(path, &s.name));
        col.entity(&e, Some(&EvidenceRef::from_block(b)));
    }
    for (pkg, l) in &out.imports {
        let Some(b) = block_at(*l) else { continue };
        col.entity(
            &thing(DEPENDENCY, clip(pkg, 120)),
            Some(&EvidenceRef::from_block(b)),
        );
    }
    let _ = SYMBOL_BP;
}
