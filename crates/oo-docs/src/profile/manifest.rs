//! Package manifests (D89): `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `requirements.txt`.
//!
//! The package the manifest declares is a `Package`; every dependency is a `Dependency` (the same name any source file
//! imports, so manifest and code share the entity), listed on the package as `attr.depends_on`. Line scanners over the
//! few well-known sections: no TOML or YAML dependency, no guessing.

use oo_core::docir::{Block, DocIr};
use serde_json::Value;

use super::{DEPENDENCY, PACKAGE, attr, clip, thing};
use crate::extract::model::{Collector, EvidenceRef};

const MAX_DEPS: usize = 150;

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase()
}

/// The source text, blocks joined in file order, and the block that holds each text offset.
fn joined(ir: &DocIr) -> (String, Vec<(usize, usize)>) {
    let mut s = String::new();
    let mut spans = Vec::new();
    for (i, b) in ir.blocks.iter().enumerate() {
        let start = s.len();
        s.push_str(&b.text);
        s.push('\n');
        spans.push((start, i));
    }
    (s, spans)
}

fn block_of<'a>(ir: &'a DocIr, spans: &[(usize, usize)], offset: usize) -> Option<&'a Block> {
    spans
        .iter()
        .rev()
        .find(|(s, _)| *s <= offset)
        .map(|(_, i)| &ir.blocks[*i])
}

fn emit(
    col: &mut Collector,
    pkg: Option<&crate::extract::model::EntityRef>,
    dep: &str,
    ev: &EvidenceRef,
) {
    let dep = clip(dep, 120);
    if dep.is_empty() {
        return;
    }
    col.entity(&thing(DEPENDENCY, dep.clone()), Some(ev));
    if let Some(p) = pkg {
        attr(col, p, "depends_on", &dep, "code.manifest", 9500, ev);
    }
}

fn package_json(ir: &DocIr, col: &mut Collector, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(first) = ir.blocks.first() else {
        return;
    };
    let ev = EvidenceRef::from_block(first);
    let pkg = v
        .get("name")
        .and_then(Value::as_str)
        .map(|n| thing(PACKAGE, clip(n, 120)));
    if let Some(p) = &pkg {
        col.entity(p, Some(&ev));
        if let Some(ver) = v.get("version").and_then(Value::as_str) {
            attr(col, p, "version", ver, "code.manifest", 9500, &ev);
        }
    }
    let mut n = 0;
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(o) = v.get(section).and_then(Value::as_object) {
            for k in o.keys() {
                if n >= MAX_DEPS {
                    return;
                }
                n += 1;
                emit(col, pkg.as_ref(), k, &ev);
            }
        }
    }
}

fn dep_name_of_toml_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') || l.starts_with('[') {
        return None;
    }
    let (k, _) = l.split_once('=')?;
    let k = k.trim().trim_matches('"');
    (!k.is_empty() && !k.contains(' ') && !k.contains('.')).then(|| k.to_owned())
}

fn cargo_toml(ir: &DocIr, col: &mut Collector, text: &str) {
    let (_, spans) = joined(ir);
    let mut section = String::new();
    let mut pkg = None;
    let mut n = 0;
    let mut offset = 0usize;
    for line in text.split('\n') {
        let l = line.trim();
        let here = offset;
        offset += line.len() + 1;
        if l.starts_with('[') {
            section = l.trim_matches(|c| c == '[' || c == ']').to_owned();
            // `[dependencies.serde]` style: the table name is the dependency.
            for prefix in [
                "dependencies.",
                "dev-dependencies.",
                "build-dependencies.",
                "workspace.dependencies.",
            ] {
                if let Some(name) = section.strip_prefix(prefix)
                    && let Some(b) = block_of(ir, &spans, here)
                    && n < MAX_DEPS
                {
                    n += 1;
                    emit(col, pkg.as_ref(), name, &EvidenceRef::from_block(b));
                }
            }
            continue;
        }
        let Some(b) = block_of(ir, &spans, here) else {
            continue;
        };
        let ev = EvidenceRef::from_block(b);
        if section == "package" && l.starts_with("name") {
            if let Some((_, v)) = l.split_once('=') {
                let name = v.trim().trim_matches('"');
                let p = thing(PACKAGE, clip(name, 120));
                col.entity(&p, Some(&ev));
                pkg = Some(p);
            }
        } else if matches!(
            section.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies" | "workspace.dependencies"
        ) && n < MAX_DEPS
            && let Some(name) = dep_name_of_toml_line(l)
        {
            n += 1;
            emit(col, pkg.as_ref(), &name, &ev);
        }
    }
}

fn requirement_name(spec: &str) -> Option<String> {
    let s = spec
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == ',');
    if s.is_empty() || s.starts_with('#') || s.starts_with('-') {
        return None;
    }
    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    (!name.is_empty()).then(|| name.to_ascii_lowercase().replace('_', "-"))
}

fn pyproject(ir: &DocIr, col: &mut Collector, text: &str) {
    let (_, spans) = joined(ir);
    let mut section = String::new();
    let mut pkg = None;
    let mut in_deps = false;
    let mut n = 0;
    let mut offset = 0usize;
    for line in text.split('\n') {
        let l = line.trim();
        let here = offset;
        offset += line.len() + 1;
        let Some(b) = block_of(ir, &spans, here) else {
            continue;
        };
        let ev = EvidenceRef::from_block(b);
        if l.starts_with('[') && !in_deps {
            section = l.trim_matches(|c| c == '[' || c == ']').to_owned();
            continue;
        }
        if in_deps {
            if l.starts_with(']') {
                in_deps = false;
                continue;
            }
            if n < MAX_DEPS
                && let Some(name) = requirement_name(l)
            {
                n += 1;
                emit(col, pkg.as_ref(), &name, &ev);
            }
            continue;
        }
        if matches!(section.as_str(), "project" | "tool.poetry") && l.starts_with("name") {
            if let Some((_, v)) = l.split_once('=') {
                let p = thing(PACKAGE, clip(v.trim().trim_matches('"'), 120));
                col.entity(&p, Some(&ev));
                pkg = Some(p);
            }
        } else if section == "project" && l.starts_with("dependencies") && l.contains('[') {
            let inline = l.split_once('[').map_or("", |(_, r)| r);
            if inline.contains(']') {
                for part in inline.split(']').next().unwrap_or("").split(',') {
                    if n < MAX_DEPS
                        && let Some(name) = requirement_name(part)
                    {
                        n += 1;
                        emit(col, pkg.as_ref(), &name, &ev);
                    }
                }
            } else {
                in_deps = true;
            }
        } else if matches!(
            section.as_str(),
            "tool.poetry.dependencies" | "tool.poetry.group.dev.dependencies"
        ) && n < MAX_DEPS
            && let Some(name) = dep_name_of_toml_line(l)
            && name != "python"
        {
            n += 1;
            emit(col, pkg.as_ref(), &name, &ev);
        }
    }
}

fn go_mod(ir: &DocIr, col: &mut Collector, text: &str) {
    let (_, spans) = joined(ir);
    let mut pkg = None;
    let mut block = false;
    let mut n = 0;
    let mut offset = 0usize;
    for line in text.split('\n') {
        let l = line.trim();
        let here = offset;
        offset += line.len() + 1;
        let Some(b) = block_of(ir, &spans, here) else {
            continue;
        };
        let ev = EvidenceRef::from_block(b);
        if let Some(m) = l.strip_prefix("module ") {
            let p = thing(PACKAGE, clip(m.trim(), 120));
            col.entity(&p, Some(&ev));
            pkg = Some(p);
        } else if l.starts_with("require (") {
            block = true;
        } else if block && l.starts_with(')') {
            block = false;
        } else if (block || l.starts_with("require ")) && n < MAX_DEPS {
            let spec = l.strip_prefix("require ").unwrap_or(l);
            if let Some(name) = spec.split_whitespace().next()
                && name.contains('.')
            {
                n += 1;
                emit(col, pkg.as_ref(), name, &ev);
            }
        }
    }
}

fn requirements(ir: &DocIr, col: &mut Collector, text: &str) {
    let (_, spans) = joined(ir);
    let mut n = 0;
    let mut offset = 0usize;
    for line in text.split('\n') {
        let here = offset;
        offset += line.len() + 1;
        let Some(b) = block_of(ir, &spans, here) else {
            continue;
        };
        if n < MAX_DEPS
            && let Some(name) = requirement_name(line)
        {
            n += 1;
            emit(col, None, &name, &EvidenceRef::from_block(b));
        }
    }
}

pub(crate) fn extract(path: &str, ir: &DocIr, col: &mut Collector) {
    match file_name(path).as_str() {
        "package.json" => package_json(ir, col, &joined(ir).0),
        "cargo.toml" => cargo_toml(ir, col, &joined(ir).0),
        "pyproject.toml" => pyproject(ir, col, &joined(ir).0),
        "go.mod" => go_mod(ir, col, &joined(ir).0),
        "requirements.txt" => requirements(ir, col, &joined(ir).0),
        _ => {}
    }
}
