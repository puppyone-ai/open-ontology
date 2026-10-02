#![forbid(unsafe_code)]
//! Independent cfg and feature walk for the cargo-deny 0.20.2 / krates 0.21.2
//! graph. This binary does not call the a117 meta_facts probe.
//! Package identity is exact string equality. Cargo 1.94 ids contain no spaces.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

#[derive(Clone, Debug)]
enum Val {
    Null,
    Bool(bool),
    Num,
    Str(String),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

struct Parser<'a> {
    buf: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn parse(buf: &'a [u8]) -> Result<Val, String> {
        let mut p = Parser { buf, i: 0 };
        let v = p.val()?;
        p.skip();
        if p.i != p.buf.len() {
            return Err(format!("trailing bytes at {}", p.i));
        }
        Ok(v)
    }

    fn skip(&mut self) {
        while let Some(b) = self.buf.get(self.i) {
            if b.is_ascii_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Result<u8, String> {
        self.skip();
        self.buf.get(self.i).copied().ok_or_else(|| "eof".to_string())
    }

    fn bump(&mut self) -> Result<u8, String> {
        let b = self.peek()?;
        self.i += 1;
        Ok(b)
    }

    fn val(&mut self) -> Result<Val, String> {
        match self.peek()? {
            b'n' => self.lit(b"null", Val::Null),
            b't' => self.lit(b"true", Val::Bool(true)),
            b'f' => self.lit(b"false", Val::Bool(false)),
            b'"' => Ok(Val::Str(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            other => Err(format!("bad value {other} at {}", self.i)),
        }
    }

    fn lit(&mut self, expect: &[u8], v: Val) -> Result<Val, String> {
        if self.buf[self.i..].starts_with(expect) {
            self.i += expect.len();
            Ok(v)
        } else {
            Err(format!("bad literal at {}", self.i))
        }
    }

    fn number(&mut self) -> Result<Val, String> {
        let start = self.i;
        while let Some(b) = self.buf.get(self.i) {
            if b.is_ascii_digit() || matches!(*b, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return Err("number".into());
        }
        Ok(Val::Num)
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bump()? != b'"' {
            return Err("string".into());
        }
        let mut out = String::new();
        loop {
            let b = self.buf.get(self.i).copied().ok_or("eof in string")?;
            self.i += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.buf.get(self.i).copied().ok_or("eof in escape")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            if self.i + 4 > self.buf.len() {
                                return Err("short unicode".into());
                            }
                            let hex = std::str::from_utf8(&self.buf[self.i..self.i + 4])
                                .map_err(|e| e.to_string())?;
                            self.i += 4;
                            let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            let ch = char::from_u32(cp).ok_or_else(|| format!("bad unicode {hex}"))?;
                            out.push(ch);
                        }
                        other => return Err(format!("bad escape {other}")),
                    }
                }
                _ => out.push(b as char),
            }
        }
    }

    fn array(&mut self) -> Result<Val, String> {
        self.bump()?;
        let mut xs = Vec::new();
        if self.peek()? == b']' {
            self.i += 1;
            return Ok(Val::Arr(xs));
        }
        loop {
            xs.push(self.val()?);
            match self.peek()? {
                b',' => self.i += 1,
                b']' => {
                    self.i += 1;
                    break;
                }
                other => return Err(format!("array {other}")),
            }
        }
        Ok(Val::Arr(xs))
    }

    fn object(&mut self) -> Result<Val, String> {
        self.bump()?;
        let mut xs = Vec::new();
        if self.peek()? == b'}' {
            self.i += 1;
            return Ok(Val::Obj(xs));
        }
        loop {
            if self.peek()? != b'"' {
                return Err("object key".into());
            }
            let k = self.string()?;
            if self.bump()? != b':' {
                return Err("object colon".into());
            }
            let v = self.val()?;
            xs.push((k, v));
            match self.peek()? {
                b',' => self.i += 1,
                b'}' => {
                    self.i += 1;
                    break;
                }
                other => return Err(format!("object {other}")),
            }
        }
        Ok(Val::Obj(xs))
    }
}

fn obj<'a>(v: &'a Val) -> Option<&'a [(String, Val)]> {
    match v {
        Val::Obj(xs) => Some(xs),
        _ => None,
    }
}

fn arr<'a>(v: &'a Val) -> Option<&'a [Val]> {
    match v {
        Val::Arr(xs) => Some(xs),
        _ => None,
    }
}

fn as_str(v: &Val) -> Option<&str> {
    match v {
        Val::Str(s) => Some(s),
        _ => None,
    }
}

fn key<'a>(o: &'a [(String, Val)], k: &str) -> Option<&'a Val> {
    o.iter().find(|(name, _)| name == k).map(|(_, v)| v)
}

fn eq_name(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.chars()
        .zip(b.chars())
        .all(|(x, y)| x == y || (x == '_' && y == '-') || (x == '-' && y == '_'))
}

#[derive(Clone, Debug)]
struct ManDep {
    name: String,
    rename: Option<String>,
    kind: String,
    target: Option<String>,
    optional: bool,
    uses_default: bool,
    features: Vec<String>,
}

#[derive(Clone, Debug)]
struct RKind {
    kind: String,
    target: Option<String>,
}

#[derive(Clone, Debug)]
struct RDep {
    name: String,
    pkg: String,
    kinds: Vec<RKind>,
}

#[derive(Clone, Debug)]
struct Pkg {
    name: String,
    version: String,
    id: String,
    deps: Vec<ManDep>,
    features: BTreeMap<String, Vec<String>>,
    optional_names: BTreeSet<String>,
    rdeps: Vec<RDep>,
    resolve_features: Vec<String>,
    in_resolve: bool,
    workspace: bool,
}

struct Model {
    pkgs: Vec<Pkg>,
    by_id: BTreeMap<String, usize>,
    roots: Vec<usize>,
}

fn kind_of(v: &Val) -> String {
    match v {
        Val::Null => "normal".into(),
        Val::Str(s) if s.is_empty() => "normal".into(),
        Val::Str(s) => s.clone(),
        _ => "normal".into(),
    }
}

fn target_of(v: &Val) -> Option<String> {
    match v {
        Val::Null => None,
        Val::Str(s) if s.is_empty() => None,
        Val::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn load_model(doc: &Val) -> Result<Model, String> {
    let root = obj(doc).ok_or("root")?;
    let packages = arr(key(root, "packages").ok_or("packages")?).ok_or("packages arr")?;
    let resolve = obj(key(root, "resolve").ok_or("resolve")?).ok_or("resolve obj")?;
    let members = arr(key(root, "workspace_members").ok_or("members")?).ok_or("members arr")?;
    let mut member_ids = BTreeSet::new();
    for m in members {
        member_ids.insert(as_str(m).ok_or("member id")?.to_string());
    }
    let mut pkgs = Vec::new();
    let mut by_id = BTreeMap::new();
    for p in packages {
        let o = obj(p).ok_or("pkg")?;
        let id = as_str(key(o, "id").ok_or("id")?).ok_or("id str")?.to_string();
        let name = as_str(key(o, "name").ok_or("name")?).ok_or("name str")?.to_string();
        let version = as_str(key(o, "version").ok_or("version")?)
            .ok_or("version str")?
            .to_string();
        let mut deps = Vec::new();
        let mut optional_names = BTreeSet::new();
        if let Some(ds) = key(o, "dependencies").and_then(arr) {
            for d in ds {
                let od = obj(d).ok_or("dep")?;
                let dep_name = as_str(key(od, "name").ok_or("dep name")?)
                    .ok_or("dep name str")?
                    .to_string();
                let optional = key(od, "optional").and_then(|v| match v {
                    Val::Bool(b) => Some(*b),
                    _ => None,
                });
                let optional = optional.unwrap_or(false);
                if optional {
                    optional_names.insert(dep_name.clone());
                }
                let uses_default = key(od, "uses_default_features")
                    .and_then(|v| match v {
                        Val::Bool(b) => Some(*b),
                        _ => None,
                    })
                    .unwrap_or(true);
                let mut features = Vec::new();
                if let Some(fs) = key(od, "features").and_then(arr) {
                    for f in fs {
                        features.push(as_str(f).unwrap_or("").to_string());
                    }
                }
                let rename = key(od, "rename").and_then(as_str).map(|s| s.to_string());
                let kind = key(od, "kind").map(kind_of).unwrap_or_else(|| "normal".into());
                let target = key(od, "target").and_then(target_of);
                deps.push(ManDep {
                    name: dep_name,
                    rename,
                    kind,
                    target,
                    optional,
                    uses_default,
                    features,
                });
            }
        }
        let mut features = BTreeMap::new();
        if let Some(ft) = key(o, "features").and_then(obj) {
            for (fk, fv) in ft {
                let mut items = Vec::new();
                if let Some(xs) = arr(fv) {
                    for item in xs {
                        if let Some(s) = as_str(item) {
                            items.push(s.to_string());
                        }
                    }
                }
                features.insert(fk.clone(), items);
            }
        }
        let idx = pkgs.len();
        by_id.insert(id.clone(), idx);
        pkgs.push(Pkg {
            name,
            version,
            id,
            deps,
            features,
            optional_names,
            rdeps: Vec::new(),
            resolve_features: Vec::new(),
            in_resolve: false,
            workspace: false,
        });
    }
    for p in &mut pkgs {
        p.workspace = member_ids.contains(&p.id);
    }
    let nodes = arr(key(resolve, "nodes").ok_or("nodes")?).ok_or("nodes arr")?;
    for n in nodes {
        let o = obj(n).ok_or("node")?;
        let id = as_str(key(o, "id").ok_or("node id")?).ok_or("node id str")?;
        let idx = *by_id.get(id).ok_or_else(|| format!("resolve id missing from packages: {id}"))?;
        pkgs[idx].in_resolve = true;
        if let Some(fs) = key(o, "features").and_then(arr) {
            for f in fs {
                if let Some(s) = as_str(f) {
                    pkgs[idx].resolve_features.push(s.to_string());
                }
            }
        }
        if let Some(ds) = key(o, "deps").and_then(arr) {
            for d in ds {
                let od = obj(d).ok_or("rdep")?;
                let name = as_str(key(od, "name").ok_or("rdep name")?)
                    .unwrap_or("")
                    .to_string();
                let pkg = as_str(key(od, "pkg").ok_or("rdep pkg")?)
                    .ok_or("rdep pkg str")?
                    .to_string();
                let mut kinds = Vec::new();
                if let Some(ks) = key(od, "dep_kinds").and_then(arr) {
                    for k in ks {
                        let ko = obj(k).ok_or("kind")?;
                        let kind = key(ko, "kind").map(kind_of).unwrap_or_else(|| "normal".into());
                        let target = key(ko, "target").and_then(target_of);
                        kinds.push(RKind { kind, target });
                    }
                }
                if kinds.is_empty() {
                    kinds.push(RKind {
                        kind: "normal".into(),
                        target: None,
                    });
                }
                pkgs[idx].rdeps.push(RDep { name, pkg, kinds });
            }
        }
    }
    let mut roots = Vec::new();
    match key(resolve, "root") {
        Some(Val::Str(id)) => {
            if let Some(idx) = by_id.get(id) {
                roots.push(*idx);
            }
        }
        _ => {
            for id in &member_ids {
                if let Some(idx) = by_id.get(id) {
                    if pkgs[*idx].in_resolve {
                        roots.push(*idx);
                    }
                }
            }
        }
    }
    roots.sort_unstable();
    roots.dedup();
    Ok(Model { pkgs, by_id, roots })
}

#[derive(Clone, Debug)]
enum Expr {
    Bare(String),
    Kv(String, String),
    Not(Box<Expr>),
    All(Vec<Expr>),
    Any(Vec<Expr>),
}

struct CfgParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> CfgParser<'a> {
    fn parse(text: &str) -> Result<Expr, String> {
        let mut p = CfgParser {
            s: text.as_bytes(),
            i: 0,
        };
        let e = p.expr()?;
        p.skip();
        if p.i != p.s.len() {
            return Err(format!("trailing cfg at {} in {text}", p.i));
        }
        Ok(e)
    }

    fn skip(&mut self) {
        while let Some(b) = self.s.get(self.i) {
            if b.is_ascii_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    fn ident(&mut self) -> Result<String, String> {
        self.skip();
        let start = self.i;
        while let Some(b) = self.s.get(self.i) {
            if b.is_ascii_alphanumeric() || *b == b'_' {
                self.i += 1;
            } else {
                break;
            }
        }
        if start == self.i {
            return Err("missing ident".into());
        }
        Ok(std::str::from_utf8(&self.s[start..self.i])
            .map_err(|e| e.to_string())?
            .to_string())
    }

    fn eat_ident(&mut self, want: &str) -> bool {
        self.skip();
        let rest = &self.s[self.i..];
        if rest.starts_with(want.as_bytes()) {
            let end = want.len();
            let boundary = rest.get(end).copied();
            if boundary.is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_')) {
                self.i += end;
                return true;
            }
        }
        false
    }

    fn string(&mut self) -> Result<String, String> {
        self.skip();
        if self.peek() != Some(b'"') {
            return Err("cfg string".into());
        }
        self.i += 1;
        let start = self.i;
        while let Some(b) = self.s.get(self.i) {
            if *b == b'"' {
                let s = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
                self.i += 1;
                return Ok(s.to_string());
            }
            self.i += 1;
        }
        Err("unterminated cfg string".into())
    }

    fn list(&mut self) -> Result<Vec<Expr>, String> {
        self.skip();
        if self.peek() != Some(b'(') {
            return Err("list".into());
        }
        self.i += 1;
        let mut xs = Vec::new();
        self.skip();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(xs);
        }
        loop {
            xs.push(self.expr()?);
            self.skip();
            if self.peek() == Some(b',') {
                self.i += 1;
                continue;
            }
            break;
        }
        self.skip();
        if self.peek() != Some(b')') {
            return Err("list end".into());
        }
        self.i += 1;
        Ok(xs)
    }

    fn expr(&mut self) -> Result<Expr, String> {
        if self.eat_ident("cfg") {
            self.skip();
            if self.peek() != Some(b'(') {
                return Err("cfg paren".into());
            }
            self.i += 1;
            let e = self.expr()?;
            self.skip();
            if self.peek() != Some(b')') {
                return Err("cfg end".into());
            }
            self.i += 1;
            return Ok(e);
        }
        if self.eat_ident("not") {
            let xs = self.list()?;
            if xs.len() != 1 {
                return Err("not arity".into());
            }
            return Ok(Expr::Not(Box::new(xs.into_iter().next().unwrap())));
        }
        if self.eat_ident("all") {
            return Ok(Expr::All(self.list()?));
        }
        if self.eat_ident("any") {
            return Ok(Expr::Any(self.list()?));
        }
        let name = self.ident()?;
        self.skip();
        if self.peek() == Some(b'=') {
            self.i += 1;
            let val = self.string()?;
            Ok(Expr::Kv(name, val))
        } else {
            Ok(Expr::Bare(name))
        }
    }
}

#[derive(Clone, Debug)]
struct Triple {
    name: String,
    arch: &'static str,
    os: &'static str,
    env: &'static str,
    vendor: &'static str,
    endian: &'static str,
    width: &'static str,
    families: &'static [&'static str],
}

fn known_triple(name: &str) -> Result<Triple, String> {
    let t = match name {
        "aarch64-unknown-linux-gnu" => Triple {
            name: name.into(),
            arch: "aarch64",
            os: "linux",
            env: "gnu",
            vendor: "unknown",
            endian: "little",
            width: "64",
            families: &["unix"],
        },
        "x86_64-unknown-linux-gnu" => Triple {
            name: name.into(),
            arch: "x86_64",
            os: "linux",
            env: "gnu",
            vendor: "unknown",
            endian: "little",
            width: "64",
            families: &["unix"],
        },
        "wasm32-unknown-unknown" => Triple {
            name: name.into(),
            arch: "wasm32",
            os: "unknown",
            env: "",
            vendor: "unknown",
            endian: "little",
            width: "32",
            families: &["wasm"],
        },
        other => return Err(format!("oracle has no cfg table for {other}")),
    };
    Ok(t)
}

fn eval_expr(expr: &Expr, t: &Triple, unknown: &mut BTreeSet<String>) -> bool {
    match expr {
        Expr::Not(inner) => !eval_expr(inner, t, unknown),
        Expr::All(xs) => xs.iter().all(|e| eval_expr(e, t, unknown)),
        Expr::Any(xs) => xs.iter().any(|e| eval_expr(e, t, unknown)),
        Expr::Bare(name) => match name.as_str() {
            "unix" => t.families.contains(&"unix"),
            "windows" => t.families.contains(&"windows"),
            other => {
                unknown.insert(other.to_string());
                false
            }
        },
        Expr::Kv(k, v) => match k.as_str() {
            "target_arch" => t.arch == v,
            "target_os" => t.os == v,
            "target_env" => t.env == v,
            "target_vendor" => t.vendor == v,
            "target_endian" => t.endian == v,
            "target_pointer_width" => t.width == v,
            "target_family" => t.families.iter().any(|f| *f == v),
            other => {
                unknown.insert(format!("{other}={v}"));
                false
            }
        },
    }
}

enum Mode {
    All,
    List(Vec<Triple>),
}

fn cfg_ok(spec: &Option<String>, mode: &Mode, unknown: &mut BTreeSet<String>) -> Result<bool, String> {
    let Some(spec) = spec else {
        return Ok(true);
    };
    match mode {
        Mode::All => Ok(true),
        Mode::List(list) => {
            let trimmed = spec.trim();
            if trimmed.starts_with("cfg(")
                || trimmed.starts_with("not(")
                || trimmed.starts_with("all(")
                || trimmed.starts_with("any(")
            {
                let expr = CfgParser::parse(trimmed)?;
                Ok(list.iter().any(|t| eval_expr(&expr, t, unknown)))
            } else {
                Ok(list.iter().any(|t| t.name == trimmed))
            }
        }
    }
}

#[derive(Clone)]
struct Extra {
    dep: String,
    feat: String,
    weak: bool,
}

struct Walk {
    reached: BTreeSet<usize>,
    enabled: Vec<BTreeSet<String>>,
    optional_on: Vec<BTreeSet<String>>,
    extras: Vec<Vec<Extra>>,
    unknown: BTreeSet<String>,
    unmatched: usize,
}

fn split_dep_feat(item: &str) -> Option<(bool, &str, &str)> {
    if let Some(i) = item.find("?/") {
        let dep = &item[..i];
        let feat = &item[i + 2..];
        if !dep.is_empty() && !feat.is_empty() {
            return Some((true, dep, feat));
        }
    } else if let Some(i) = item.find('/') {
        if !item.starts_with("dep:") {
            let dep = &item[..i];
            let feat = &item[i + 1..];
            if !dep.is_empty() && !feat.is_empty() {
                return Some((false, dep, feat));
            }
        }
    }
    None
}

impl Walk {
    fn new(n: usize) -> Self {
        Self {
            reached: BTreeSet::new(),
            enabled: vec![BTreeSet::new(); n],
            optional_on: vec![BTreeSet::new(); n],
            extras: vec![Vec::new(); n],
            unknown: BTreeSet::new(),
            unmatched: 0,
        }
    }

    fn enable_feature(&mut self, idx: usize, feat: &str, pkgs: &[Pkg]) -> bool {
        if let Some(name) = feat.strip_prefix("dep:") {
            return self.optional_on[idx].insert(name.to_string());
        }
        if feat.is_empty() || !self.enabled[idx].insert(feat.to_string()) {
            return false;
        }
        let items = pkgs[idx].features.get(feat).cloned().unwrap_or_default();
        for item in items {
            self.apply_item(idx, &item, pkgs);
        }
        if pkgs[idx].optional_names.contains(feat) {
            self.optional_on[idx].insert(feat.to_string());
        }
        true
    }

    fn apply_item(&mut self, idx: usize, item: &str, pkgs: &[Pkg]) {
        if let Some((weak, dep, feat)) = split_dep_feat(item) {
            self.extras[idx].push(Extra {
                dep: dep.to_string(),
                feat: feat.to_string(),
                weak,
            });
            return;
        }
        if let Some(name) = item.strip_prefix("dep:") {
            self.optional_on[idx].insert(name.to_string());
            return;
        }
        if pkgs[idx].features.contains_key(item) {
            self.enable_feature(idx, item, pkgs);
        } else {
            self.optional_on[idx].insert(item.to_string());
        }
    }
}

fn targets_match(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn find_man<'a>(pkg: &'a Pkg, rname: &str, child_name: &str, kind: &str, target: &Option<String>) -> Option<&'a ManDep> {
    pkg.deps.iter().find(|d| {
        d.kind == kind
            && targets_match(&d.target, target)
            && (eq_name(&d.name, rname)
                || eq_name(&d.name, child_name)
                || d.rename.as_deref().is_some_and(|r| eq_name(r, rname)))
    })
}

fn dep_activated(walk: &Walk, idx: usize, dep_name: &str) -> bool {
    walk.optional_on[idx].iter().any(|n| eq_name(n, dep_name))
        || walk.enabled[idx].iter().any(|n| eq_name(n, dep_name))
}

struct Outcome {
    kept: BTreeSet<(String, String)>,
    notes: String,
}

fn evaluate(model: &Model, mode: &Mode) -> Result<Outcome, String> {
    let mut walk = Walk::new(model.pkgs.len());
    for &idx in &model.roots {
        walk.reached.insert(idx);
        let feats = model.pkgs[idx].resolve_features.clone();
        for f in feats {
            walk.enable_feature(idx, &f, &model.pkgs);
        }
    }
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 10000 {
            return Err("fixpoint did not settle".into());
        }
        let mut changed = false;
        let current: Vec<usize> = walk.reached.iter().copied().collect();
        for idx in current {
            for rdep in &model.pkgs[idx].rdeps {
                let child = match model.by_id.get(&rdep.pkg) {
                    Some(c) => *c,
                    None => {
                        walk.unmatched += 1;
                        continue;
                    }
                };
                let child_name = model.pkgs[child].name.clone();
                for kind in &rdep.kinds {
                    if kind.kind == "dev" && !model.pkgs[idx].workspace {
                        continue;
                    }
                    if !cfg_ok(&kind.target, mode, &mut walk.unknown)? {
                        continue;
                    }
                    let man = find_man(&model.pkgs[idx], &rdep.name, &child_name, &kind.kind, &kind.target);
                    let (optional, uses_default, features, man_name) = if let Some(man) = man {
                        (
                            man.optional,
                            man.uses_default,
                            man.features.clone(),
                            man.name.clone(),
                        )
                    } else {
                        walk.unmatched += 1;
                        (false, false, Vec::new(), rdep.name.clone())
                    };
                    if optional && !dep_activated(&walk, idx, &man_name) && !dep_activated(&walk, idx, &child_name)
                    {
                        continue;
                    }
                    if walk.reached.insert(child) {
                        changed = true;
                    }
                    for f in &features {
                        if walk.enable_feature(child, f, &model.pkgs) {
                            changed = true;
                        }
                    }
                    if uses_default && model.pkgs[child].features.contains_key("default") {
                        if walk.enable_feature(child, "default", &model.pkgs) {
                            changed = true;
                        }
                    }
                    let extras: Vec<(String, bool)> = walk.extras[idx]
                        .iter()
                        .filter(|ex| eq_name(&ex.dep, &man_name) || eq_name(&ex.dep, &child_name))
                        .filter(|ex| {
                            !ex.weak
                                || dep_activated(&walk, idx, &ex.dep)
                                || walk.enabled[idx].iter().any(|n| eq_name(n, &ex.dep))
                        })
                        .map(|ex| (ex.feat.clone(), ex.weak))
                        .collect();
                    for (feat, _) in extras {
                        if walk.enable_feature(child, &feat, &model.pkgs) {
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut kept = BTreeSet::new();
    for idx in &walk.reached {
        let p = &model.pkgs[*idx];
        kept.insert((p.name.clone(), p.version.clone()));
    }
    let mut notes = String::new();
    writeln!(notes, "kept_count={}", kept.len()).unwrap();
    writeln!(notes, "reached_ids={}", walk.reached.len()).unwrap();
    writeln!(notes, "roots={}", model.roots.len()).unwrap();
    writeln!(notes, "package_count={}", model.pkgs.len()).unwrap();
    writeln!(
        notes,
        "resolve_count={}",
        model.pkgs.iter().filter(|p| p.in_resolve).count()
    )
    .unwrap();
    writeln!(notes, "unmatched_edges={}", walk.unmatched).unwrap();
    writeln!(
        notes,
        "unknown_preds={}",
        if walk.unknown.is_empty() {
            "0".into()
        } else {
            walk.unknown.into_iter().collect::<Vec<_>>().join(",")
        }
    )
    .unwrap();
    for idx in &walk.reached {
        let p = &model.pkgs[*idx];
        if matches!(
            p.name.as_str(),
            "ahash" | "arrow-array" | "rustls-platform-verifier" | "const-random" | "tiny-keccak"
        ) {
            let feats: Vec<&str> = walk.enabled[*idx].iter().map(|s| s.as_str()).collect();
            writeln!(
                notes,
                "enabled\t{}\t{}\t{}",
                p.name,
                p.version,
                feats.join(",")
            )
            .unwrap();
        }
    }
    for name in [
        "tiny-keccak",
        "webpki-root-certs",
        "const-random",
        "const-random-macro",
        "core_detect",
        "multiversion",
        "multiversion-macros",
        "rustversion",
        "arrow-array",
        "rustls-native-certs",
        "ahash",
        "getrandom",
    ] {
        let present = kept.iter().any(|(n, _)| n == name);
        writeln!(
            notes,
            "marker\t{name}\t{}",
            if present { "kept" } else { "absent" }
        )
        .unwrap();
        for p in &model.pkgs {
            if p.name == name {
                writeln!(
                    notes,
                    "in_resolve\t{}\t{}\t{}",
                    p.name,
                    p.version,
                    if p.in_resolve { "yes" } else { "no" }
                )
                .unwrap();
            }
        }
    }
    Ok(Outcome { kept, notes })
}

fn write_kept(path: &str, kept: &BTreeSet<(String, String)>) -> Result<(), String> {
    let mut buf = String::new();
    for (n, v) in kept {
        writeln!(buf, "kept\t{n}\t{v}").unwrap();
    }
    fs::write(path, buf).map_err(|e| e.to_string())
}

fn read_kept(path: &str) -> Result<BTreeSet<(String, String)>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut set = BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("kept\t") {
            let mut parts = rest.split('\t');
            let n = parts.next().unwrap_or("");
            let v = parts.next().unwrap_or("");
            if !n.is_empty() {
                set.insert((n.to_string(), v.to_string()));
            }
        }
    }
    Ok(set)
}

fn parse_filtered(text: &str) -> Result<(BTreeSet<(String, String)>, Option<usize>), String> {
    let mut filtered = BTreeSet::new();
    let mut gathered = None;
    for line in text.lines() {
        let msg = if let Some(i) = line.find("\"message\":\"") {
            let rest = &line[i + "\"message\":\"".len()..];
            let end = rest.find('"').ok_or("message end")?;
            rest[..end].replace("\\\"", "\"")
        } else if let Some(i) = line.find("] ") {
            line[i + 2..].to_string()
        } else {
            line.to_string()
        };
        if let Some(rest) = msg.strip_prefix("filtered ") {
            let mut parts = rest.split_whitespace();
            let name = parts.next().unwrap_or("");
            let ver = parts.next().unwrap_or("");
            if !name.is_empty() && !ver.is_empty() {
                filtered.insert((name.to_string(), ver.to_string()));
            }
        }
        if let Some(rest) = msg.strip_prefix("gathered ") {
            if let Some(n) = rest.split_whitespace().next() {
                if let Ok(v) = n.parse::<usize>() {
                    if rest.contains("crates") {
                        gathered = Some(v);
                    }
                }
            }
        }
    }
    Ok((filtered, gathered))
}

fn mode_from_args(args: &[String]) -> Result<Mode, String> {
    if args.iter().any(|a| a == "--all-targets") {
        return Ok(Mode::All);
    }
    let mut triples = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--targets" {
            let spec = args.get(i + 1).ok_or("--targets needs a value")?;
            for part in spec.split(',') {
                if !part.is_empty() {
                    triples.push(known_triple(part)?);
                }
            }
        }
        i += 1;
    }
    if triples.is_empty() {
        return Err("pass --all-targets or --targets a,b".into());
    }
    Ok(Mode::List(triples))
}

fn names_of(set: &BTreeSet<(String, String)>) -> BTreeSet<String> {
    set.iter().map(|(n, _)| n.clone()).collect()
}

fn self_test() -> Result<(), String> {
    let id = "registry+https://github.com/rust-lang/crates.io-index#ahash@0.8.12";
    if id.split_whitespace().count() != 1 || id == "ahash" {
        return Err("id fixture".into());
    }
    let linux = known_triple("aarch64-unknown-linux-gnu")?;
    let wasm = known_triple("wasm32-unknown-unknown")?;
    let expr = CfgParser::parse(
        "cfg(all(unix, not(target_os = \"android\"), not(target_vendor = \"apple\"), not(target_arch = \"wasm32\")))",
    )?;
    let mut unknown = BTreeSet::new();
    if !eval_expr(&expr, &linux, &mut unknown) || eval_expr(&expr, &wasm, &mut unknown) {
        return Err(format!("native cfg eval unknown={unknown:?}"));
    }
    let x_only = CfgParser::parse("cfg(target_arch = \"x86_64\")")?;
    let x64 = known_triple("x86_64-unknown-linux-gnu")?;
    if eval_expr(&x_only, &linux, &mut unknown) || !eval_expr(&x_only, &x64, &mut unknown) {
        return Err("x86_64 cfg".into());
    }
    let doc = Parser::parse(FIXTURE.as_bytes())?;
    let model = load_model(&doc)?;
    let union = evaluate(
        &model,
        &Mode::List(vec![
            known_triple("aarch64-unknown-linux-gnu")?,
            known_triple("x86_64-unknown-linux-gnu")?,
        ]),
    )?;
    let arm = evaluate(
        &model,
        &Mode::List(vec![known_triple("aarch64-unknown-linux-gnu")?]),
    )?;
    let wasm_set = evaluate(&model, &Mode::List(vec![known_triple("wasm32-unknown-unknown")?]))?;
    let all = evaluate(&model, &Mode::All)?;
    let u = names_of(&union.kept);
    let a = names_of(&arm.kept);
    let w = names_of(&wasm_set.kept);
    let z = names_of(&all.kept);
    let need = |set: &BTreeSet<String>, name: &str, want: bool| -> Result<(), String> {
        if set.contains(name) != want {
            Err(format!("{name} present={} want={want} in {set:?}", set.contains(name)))
        } else {
            Ok(())
        }
    };
    for (label, set) in [("union", &u), ("arm", &a), ("wasm", &w), ("all", &z)] {
        if set.contains("devonly") {
            return Err(format!("{label} kept devonly"));
        }
    }
    need(&u, "tiny-keccak", false)?;
    need(&u, "webpki-root-certs", false)?;
    need(&u, "const-random", false)?;
    need(&u, "core_detect", true)?;
    need(&u, "rustls-native-certs", true)?;
    need(&u, "getrandom", true)?;
    need(&u, "ahash", true)?;
    need(&a, "core_detect", false)?;
    need(&a, "tiny-keccak", false)?;
    need(&a, "rustls-native-certs", true)?;
    need(&w, "tiny-keccak", true)?;
    need(&w, "const-random", true)?;
    need(&w, "webpki-root-certs", true)?;
    need(&w, "rustls-native-certs", false)?;
    need(&w, "core_detect", false)?;
    need(&w, "getrandom", false)?;
    need(&z, "tiny-keccak", true)?;
    need(&z, "webpki-root-certs", true)?;
    need(&z, "core_detect", true)?;
    need(&z, "rustls-native-certs", true)?;
    if !union.notes.contains("marker\ttiny-keccak\tabsent") {
        return Err(union.notes);
    }
    if union.notes.contains("enabled\tahash\t0.8.12\t")
        && union.notes.lines().any(|l| l.starts_with("enabled\tahash") && l.contains("compile-time-rng"))
    {
        return Err(format!("union ahash enabled compile-time-rng\n{}", union.notes));
    }
    Ok(())
}

const FIXTURE: &str = r#"{"packages":[
{"name":"app","version":"0.1.0","id":"app","dependencies":[
{"name":"arrow","kind":null,"optional":false,"uses_default_features":false,"features":[],"target":null},
{"name":"core_detect","kind":null,"optional":false,"uses_default_features":false,"features":[],"target":"cfg(target_arch = \"x86_64\")"},
{"name":"verifier","kind":null,"optional":false,"uses_default_features":false,"features":[],"target":null}
],"features":{}},
{"name":"arrow","version":"1.0.0","id":"arrow","dependencies":[
{"name":"ahash","kind":null,"optional":false,"uses_default_features":false,"features":["compile-time-rng"],"target":"cfg(target_arch = \"wasm32\")"},
{"name":"ahash","kind":null,"optional":false,"uses_default_features":false,"features":["runtime-rng"],"target":"cfg(not(target_arch = \"wasm32\"))"}
],"features":{}},
{"name":"ahash","version":"0.8.12","id":"ahash","dependencies":[
{"name":"const-random","kind":null,"optional":true,"uses_default_features":true,"features":[],"target":null},
{"name":"getrandom","kind":null,"optional":true,"uses_default_features":true,"features":[],"target":null},
{"name":"devonly","kind":"dev","optional":false,"uses_default_features":true,"features":[],"target":null}
],"features":{"default":["std","runtime-rng"],"std":[],"runtime-rng":["getrandom"],"compile-time-rng":["const-random"],"const-random":["dep:const-random"],"getrandom":["dep:getrandom"]}},
{"name":"const-random","version":"0.1.0","id":"const-random","dependencies":[
{"name":"tiny-keccak","kind":null,"optional":false,"uses_default_features":true,"features":["shake"],"target":null}
],"features":{"default":[]}},
{"name":"tiny-keccak","version":"2.0.2","id":"tiny-keccak","dependencies":[],"features":{"default":[],"shake":[]}},
{"name":"getrandom","version":"0.2.0","id":"getrandom","dependencies":[],"features":{}},
{"name":"devonly","version":"1.0.0","id":"devonly","dependencies":[],"features":{}},
{"name":"core_detect","version":"1.0.0","id":"core_detect","dependencies":[],"features":{}},
{"name":"verifier","version":"0.7.0","id":"verifier","dependencies":[
{"name":"webpki-root-certs","kind":null,"optional":false,"uses_default_features":true,"features":[],"target":"cfg(target_arch = \"wasm32\")"},
{"name":"rustls-native-certs","kind":null,"optional":false,"uses_default_features":true,"features":[],"target":"cfg(all(unix, not(target_os = \"android\"), not(target_vendor = \"apple\"), not(target_arch = \"wasm32\")))"}
],"features":{}},
{"name":"webpki-root-certs","version":"1.0.9","id":"webpki-root-certs","dependencies":[],"features":{}},
{"name":"rustls-native-certs","version":"0.8.4","id":"rustls-native-certs","dependencies":[],"features":{}}
],"resolve":{"root":null,"nodes":[
{"id":"app","features":[],"deps":[
{"name":"arrow","pkg":"arrow","dep_kinds":[{"kind":null,"target":null}]},
{"name":"core_detect","pkg":"core_detect","dep_kinds":[{"kind":null,"target":"cfg(target_arch = \"x86_64\")"}]},
{"name":"verifier","pkg":"verifier","dep_kinds":[{"kind":null,"target":null}]}
]},
{"id":"arrow","features":[],"deps":[
{"name":"ahash","pkg":"ahash","dep_kinds":[
{"kind":null,"target":"cfg(target_arch = \"wasm32\")"},
{"kind":null,"target":"cfg(not(target_arch = \"wasm32\"))"}
]}
]},
{"id":"ahash","features":["compile-time-rng","const-random","default","getrandom","runtime-rng","std"],"deps":[
{"name":"const-random","pkg":"const-random","dep_kinds":[{"kind":null,"target":null}]},
{"name":"getrandom","pkg":"getrandom","dep_kinds":[{"kind":null,"target":null}]},
{"name":"devonly","pkg":"devonly","dep_kinds":[{"kind":"dev","target":null}]}
]},
{"id":"const-random","features":["default"],"deps":[
{"name":"tiny-keccak","pkg":"tiny-keccak","dep_kinds":[{"kind":null,"target":null}]}
]},
{"id":"tiny-keccak","features":["default","shake"],"deps":[]},
{"id":"getrandom","features":[],"deps":[]},
{"id":"devonly","features":[],"deps":[]},
{"id":"core_detect","features":[],"deps":[]},
{"id":"verifier","features":[],"deps":[
{"name":"webpki-root-certs","pkg":"webpki-root-certs","dep_kinds":[{"kind":null,"target":"cfg(target_arch = \"wasm32\")"}]},
{"name":"rustls-native-certs","pkg":"rustls-native-certs","dep_kinds":[{"kind":null,"target":"cfg(all(unix, not(target_os = \"android\"), not(target_vendor = \"apple\"), not(target_arch = \"wasm32\")))"}]}
]},
{"id":"webpki-root-certs","features":[],"deps":[]},
{"id":"rustls-native-certs","features":[],"deps":[]}
]},"workspace_members":["app"]}"#;

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "self-test" => self_test(),
        "eval" => {
            let meta = args.get(1).ok_or("eval METADATA OUT")?;
            let out = args.get(2).ok_or("eval METADATA OUT")?;
            let doc = Parser::parse(&fs::read(meta).map_err(|e| e.to_string())?)?;
            let model = load_model(&doc)?;
            let mode = mode_from_args(&args)?;
            let outcome = evaluate(&model, &mode)?;
            write_kept(out, &outcome.kept)?;
            fs::write(format!("{out}.notes"), &outcome.notes).map_err(|e| e.to_string())?;
            let mut stdout = io::stdout().lock();
            write!(stdout, "{}", outcome.notes).map_err(|e| e.to_string())?;
            Ok(())
        }
        "subtract" => {
            let meta = args.get(1).ok_or("subtract METADATA LOG OUT")?;
            let log = args.get(2).ok_or("subtract METADATA LOG OUT")?;
            let out = args.get(3).ok_or("subtract METADATA LOG OUT")?;
            let doc = Parser::parse(&fs::read(meta).map_err(|e| e.to_string())?)?;
            let model = load_model(&doc)?;
            let text = fs::read_to_string(log).map_err(|e| e.to_string())?;
            let (filtered, gathered) = parse_filtered(&text)?;
            let mut kept = BTreeSet::new();
            for p in &model.pkgs {
                if !filtered.contains(&(p.name.clone(), p.version.clone())) {
                    kept.insert((p.name.clone(), p.version.clone()));
                }
            }
            write_kept(out, &kept)?;
            let mut notes = String::new();
            writeln!(notes, "filtered_count={}", filtered.len()).unwrap();
            writeln!(notes, "kept_count={}", kept.len()).unwrap();
            writeln!(
                notes,
                "gathered={}",
                gathered.map(|n| n.to_string()).unwrap_or_else(|| "missing".into())
            )
            .unwrap();
            writeln!(
                notes,
                "count_match={}",
                gathered == Some(kept.len())
            )
            .unwrap();
            for name in [
                "tiny-keccak",
                "webpki-root-certs",
                "const-random",
                "const-random-macro",
                "core_detect",
                "multiversion",
                "multiversion-macros",
                "rustversion",
                "arrow-array",
                "rustls-native-certs",
            ] {
                let present = kept.iter().any(|(n, _)| n == name);
                writeln!(
                    notes,
                    "marker\t{name}\t{}",
                    if present { "kept" } else { "absent" }
                )
                .unwrap();
            }
            fs::write(format!("{out}.notes"), &notes).map_err(|e| e.to_string())?;
            print!("{notes}");
            Ok(())
        }
        "diff" => {
            let left = read_kept(args.get(1).ok_or("diff A B OUT")?)?;
            let right = read_kept(args.get(2).ok_or("diff A B OUT")?)?;
            let out = args.get(3).ok_or("diff A B OUT")?;
            let mut only_left = Vec::new();
            let mut only_right = Vec::new();
            for item in left.difference(&right) {
                only_left.push(format!("oracle_only\t{}\t{}", item.0, item.1));
            }
            for item in right.difference(&left) {
                only_right.push(format!("deny_only\t{}\t{}", item.0, item.1));
            }
            let mut buf = String::new();
            writeln!(buf, "oracle_count={}", left.len()).unwrap();
            writeln!(buf, "deny_count={}", right.len()).unwrap();
            writeln!(buf, "oracle_only={}", only_left.len()).unwrap();
            writeln!(buf, "deny_only={}", only_right.len()).unwrap();
            for line in only_left.iter().chain(only_right.iter()) {
                writeln!(buf, "{line}").unwrap();
            }
            fs::write(out, &buf).map_err(|e| e.to_string())?;
            print!("{buf}");
            if only_left.is_empty() && only_right.is_empty() {
                Ok(())
            } else {
                Err(format!("sets differ oracle_only={} deny_only={}", only_left.len(), only_right.len()))
            }
        }
        _ => Err("usage: self-test | eval | subtract | diff".into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("graph_oracle: {err}");
            ExitCode::from(1)
        }
    }
}
