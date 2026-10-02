#![forbid(unsafe_code)]
//! Read one `cargo metadata --format-version 1` document and report
//! resolve-node coverage plus target predicates for the license graph.
//! Absence is `in_resolve=no` together with the packages-array fact.
//! An empty cargo-tree is not an input to this program.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

#[derive(Clone, Debug)]
enum Val {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

struct Parser<'a> {
    buf: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, i: 0 }
    }

    fn parse(mut self) -> Result<Val, String> {
        let v = self.val()?;
        self.skip();
        if self.i != self.buf.len() {
            return Err(format!("trailing bytes at {}", self.i));
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
            other => Err(format!("bad value {} at {}", other, self.i)),
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
        if self.peek()? == b'-' {
            self.i += 1;
        }
        if self.i >= self.buf.len() || !self.buf[self.i].is_ascii_digit() {
            return Err(format!("bad number at {}", start));
        }
        while self.i < self.buf.len() && self.buf[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.i < self.buf.len() && self.buf[self.i] == b'.' {
            self.i += 1;
            while self.i < self.buf.len() && self.buf[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if self.i < self.buf.len() && (self.buf[self.i] == b'e' || self.buf[self.i] == b'E') {
            self.i += 1;
            if self.i < self.buf.len() && (self.buf[self.i] == b'+' || self.buf[self.i] == b'-') {
                self.i += 1;
            }
            while self.i < self.buf.len() && self.buf[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        let s = std::str::from_utf8(&self.buf[start..self.i]).map_err(|e| e.to_string())?;
        Ok(Val::Num(s.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bump()? != b'"' {
            return Err("string".into());
        }
        let mut out = String::new();
        loop {
            if self.i >= self.buf.len() {
                return Err("unterminated string".into());
            }
            let b = self.buf[self.i];
            self.i += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.buf.get(self.i).copied().ok_or("bad escape")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            if self.i + 4 > self.buf.len() {
                                return Err("bad unicode".into());
                            }
                            let hex = std::str::from_utf8(&self.buf[self.i..self.i + 4])
                                .map_err(|err| err.to_string())?;
                            let code = u32::from_str_radix(hex, 16).map_err(|err| err.to_string())?;
                            let ch = char::from_u32(code).ok_or("non-scalar unicode")?;
                            out.push(ch);
                            self.i += 4;
                        }
                        _ => return Err(format!("bad escape {}", e)),
                    }
                }
                c => out.push(char::from(c)),
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
            match self.bump()? {
                b']' => return Ok(Val::Arr(xs)),
                b',' => continue,
                _ => return Err(format!("array at {}", self.i)),
            }
        }
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
                return Err(format!("key at {}", self.i));
            }
            let k = self.string()?;
            if self.bump()? != b':' {
                return Err("colon".into());
            }
            xs.push((k, self.val()?));
            match self.bump()? {
                b'}' => return Ok(Val::Obj(xs)),
                b',' => continue,
                _ => return Err(format!("object at {}", self.i)),
            }
        }
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

fn key<'a>(o: &'a [(String, Val)], name: &str) -> Option<&'a Val> {
    o.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn as_str(v: &Val) -> Option<&str> {
    match v {
        Val::Str(s) => Some(s),
        _ => None,
    }
}

fn as_bool(v: &Val) -> Option<bool> {
    match v {
        Val::Bool(b) => Some(*b),
        _ => None,
    }
}

fn disp(v: &Val) -> String {
    match v {
        Val::Null => "null".into(),
        Val::Bool(true) => "true".into(),
        Val::Bool(false) => "false".into(),
        Val::Num(s) | Val::Str(s) => s.clone(),
        Val::Arr(xs) => {
            let parts: Vec<String> = xs.iter().map(disp).collect();
            parts.join(",")
        }
        Val::Obj(_) => "<obj>".into(),
    }
}

fn kind_of(v: &Val) -> String {
    match v {
        Val::Null => "normal".into(),
        other => disp(other),
    }
}

const INTEREST: &[&str] = &[
    "ahash",
    "arrow-array",
    "arrow-buffer",
    "const-random",
    "const-random-macro",
    "getrandom",
    "oo-connectors",
    "oo-sdk",
    "reqwest",
    "rustls-native-certs",
    "rustls-platform-verifier",
    "tiny-keccak",
    "webpki-root-certs",
];

fn interesting(name: &str) -> bool {
    INTEREST.iter().any(|n| *n == name)
}

fn write_report(doc: &Val, mut out: impl Write) -> Result<(), String> {
    let root = obj(doc).ok_or("root")?;
    let packages = arr(key(root, "packages").ok_or("packages")?).ok_or("packages arr")?;
    let resolve = obj(key(root, "resolve").ok_or("resolve")?).ok_or("resolve obj")?;
    let nodes = arr(key(resolve, "nodes").ok_or("nodes")?).ok_or("nodes arr")?;

    writeln!(out, "package_count={}", packages.len()).map_err(|e| e.to_string())?;
    writeln!(out, "resolve_node_count={}", nodes.len()).map_err(|e| e.to_string())?;

    // Cargo 1.94 package ids are `registry+…#name@version` or `path+…#version`,
    // not `name version`. Membership is exact id equality with resolve nodes.
    let mut id_name: Vec<(String, String, String)> = Vec::new();
    for pkg in packages {
        let o = obj(pkg).ok_or("pkg")?;
        let id = as_str(key(o, "id").ok_or("pkg id")?).unwrap_or("").to_string();
        let name = as_str(key(o, "name").ok_or("pkg name")?).unwrap_or("").to_string();
        let version = as_str(key(o, "version").ok_or("pkg ver")?).unwrap_or("").to_string();
        id_name.push((id, name, version));
    }
    let mut resolve_rows: Vec<(String, String, String)> = Vec::new();
    let mut resolve_ids: Vec<String> = Vec::new();
    for node in nodes {
        let o = obj(node).ok_or("node")?;
        let id = as_str(key(o, "id").ok_or("id")?).ok_or("id str")?.to_string();
        let mut features: Vec<String> = key(o, "features")
            .and_then(arr)
            .map(|xs| xs.iter().map(disp).collect())
            .unwrap_or_default();
        features.sort();
        let features = features.join(",");
        let (name, version) = id_name
            .iter()
            .find(|(pid, _, _)| pid == &id)
            .map(|(_, n, v)| (n.clone(), v.clone()))
            .unwrap_or_else(|| ("?".into(), "?".into()));
        resolve_ids.push(id);
        resolve_rows.push((name, version, features));
    }
    resolve_rows.sort();
    for (name, version, features) in &resolve_rows {
        writeln!(out, "resolve_node\t{name}\t{version}\t{features}").map_err(|e| e.to_string())?;
    }

    // Resolve edges are the metadata graph. They are not the per-target cargo-tree
    // activation set: cargo keeps unified features, so a Linux filter can still
    // list a target=null edge that cargo tree does not compile.
    for node in nodes {
        let o = obj(node).ok_or("node edge")?;
        let id = as_str(key(o, "id").ok_or("edge id")?).unwrap_or("");
        let (pname, pver) = id_name
            .iter()
            .find(|(pid, _, _)| pid == id)
            .map(|(_, n, v)| (n.as_str(), v.as_str()))
            .unwrap_or(("?", "?"));
        let Some(deps) = key(o, "deps").and_then(arr) else {
            continue;
        };
        for dep in deps {
            let d = obj(dep).ok_or("rdep")?;
            let pkg_id = key(d, "pkg").and_then(as_str).unwrap_or("");
            let fallback = key(d, "name").and_then(as_str).unwrap_or("");
            let (dname, dver) = id_name
                .iter()
                .find(|(pid, _, _)| pid == pkg_id)
                .map(|(_, n, v)| (n.clone(), v.clone()))
                .unwrap_or_else(|| (fallback.to_string(), "?".into()));
            if !interesting(pname) && !interesting(&dname) {
                continue;
            }
            let Some(kinds) = key(d, "dep_kinds").and_then(arr) else {
                continue;
            };
            if kinds.is_empty() {
                writeln!(
                    out,
                    "resolve_edge\t{pname}\t{pver}\t{dname}\t{dver}\tkind=missing\ttarget=null"
                )
                .map_err(|e| e.to_string())?;
            }
            for kind in kinds {
                let ko = obj(kind).ok_or("dep_kind")?;
                let kind_s = key(ko, "kind").map(kind_of).unwrap_or_else(|| "normal".into());
                let target = key(ko, "target").map(disp).unwrap_or_else(|| "null".into());
                writeln!(
                    out,
                    "resolve_edge\t{pname}\t{pver}\t{dname}\t{dver}\tkind={kind_s}\ttarget={target}"
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }

    for pkg in packages {
        let o = obj(pkg).ok_or("pkg")?;
        let name = as_str(key(o, "name").ok_or("name")?).ok_or("name str")?;
        if !interesting(name) {
            continue;
        }
        let version = as_str(key(o, "version").ok_or("version")?).unwrap_or("");
        let id = as_str(key(o, "id").ok_or("id")?).unwrap_or("");
        let in_resolve = resolve_ids.iter().any(|rid| rid == id);
        writeln!(
            out,
            "package\t{name}\t{version}\tin_resolve={}\tid={id}",
            if in_resolve { "yes" } else { "no" }
        )
        .map_err(|e| e.to_string())?;
        if let Some(deps) = key(o, "dependencies").and_then(arr) {
            for dep in deps {
                let d = obj(dep).ok_or("dep")?;
                let dep_name = as_str(key(d, "name").ok_or("dep name")?).unwrap_or("");
                if !interesting(dep_name) && dep_name != "ahash" {
                    continue;
                }
                let kind = key(d, "kind").map(kind_of).unwrap_or_else(|| "normal".into());
                let target = key(d, "target").map(disp).unwrap_or_else(|| "null".into());
                let optional = key(d, "optional").and_then(as_bool);
                let uses_default = key(d, "uses_default_features").and_then(as_bool);
                let features = key(d, "features").map(disp).unwrap_or_default();
                writeln!(
                    out,
                    "dep\tparent={name}@{version}\tdep={dep_name}\tkind={kind}\ttarget={target}\toptional={}\tdefault_features={}\tfeatures={features}",
                    optional.map(|b| b.to_string()).unwrap_or_else(|| "null".into()),
                    uses_default.map(|b| b.to_string()).unwrap_or_else(|| "null".into()),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if let Some(feats) = key(o, "features").and_then(obj) {
            let mut names: Vec<&str> = feats.iter().map(|(k, _)| k.as_str()).collect();
            names.sort_unstable();
            writeln!(out, "feature_table\t{name}@{version}\t{}", names.join(",")).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn fixture() -> &'static str {
    r#"{"packages":[{"name":"arrow-array","version":"60.0.0","id":"registry+https://github.com/rust-lang/crates.io-index#arrow-array@60.0.0","dependencies":[{"name":"ahash","kind":null,"optional":false,"uses_default_features":false,"features":["compile-time-rng"],"target":"cfg(target_arch = \"wasm32\")"},{"name":"ahash","kind":null,"optional":false,"uses_default_features":false,"features":["runtime-rng"],"target":"cfg(not(target_arch = \"wasm32\"))"}],"features":{"async":["dep:xx"]}},{"name":"ahash","version":"0.8.12","id":"registry+https://github.com/rust-lang/crates.io-index#ahash@0.8.12","dependencies":[],"features":{"runtime-rng":["getrandom"]}},{"name":"tiny-keccak","version":"2.0.2","id":"registry+https://github.com/rust-lang/crates.io-index#tiny-keccak@2.0.2","dependencies":[],"features":{"default":[],"shake":[]}},{"name":"other","version":"1.0.0","id":"registry+https://github.com/rust-lang/crates.io-index#other@1.0.0","dependencies":[],"features":{}}],"resolve":{"nodes":[{"id":"registry+https://github.com/rust-lang/crates.io-index#arrow-array@60.0.0","features":[],"deps":[{"name":"ahash","pkg":"registry+https://github.com/rust-lang/crates.io-index#ahash@0.8.12","dep_kinds":[{"kind":null,"target":"cfg(target_arch = \"wasm32\")"},{"kind":null,"target":"cfg(not(target_arch = \"wasm32\"))"}]}]},{"id":"registry+https://github.com/rust-lang/crates.io-index#ahash@0.8.12","features":["runtime-rng","std"]}],"root":null},"workspace_members":[]}"#
}

fn self_test() -> Result<(), String> {
    let doc = Parser::new(fixture().as_bytes()).parse()?;
    let mut buf = Vec::new();
    write_report(&doc, &mut buf)?;
    let text = String::from_utf8(buf).map_err(|e| e.to_string())?;
    let need = [
        "package_count=4",
        "resolve_node_count=2",
        "resolve_node\tahash\t0.8.12\truntime-rng,std",
        "package\ttiny-keccak\t2.0.2\tin_resolve=no",
        "package\tarrow-array\t60.0.0\tin_resolve=yes",
        "package\tahash\t0.8.12\tin_resolve=yes",
        "dep\tparent=arrow-array@60.0.0\tdep=ahash\tkind=normal\ttarget=cfg(target_arch = \"wasm32\")\toptional=false\tdefault_features=false\tfeatures=compile-time-rng",
        "dep\tparent=arrow-array@60.0.0\tdep=ahash\tkind=normal\ttarget=cfg(not(target_arch = \"wasm32\"))\toptional=false\tdefault_features=false\tfeatures=runtime-rng",
        "feature_table\tahash@0.8.12\truntime-rng",
        "resolve_edge\tarrow-array\t60.0.0\tahash\t0.8.12\tkind=normal\ttarget=cfg(target_arch = \"wasm32\")",
        "resolve_edge\tarrow-array\t60.0.0\tahash\t0.8.12\tkind=normal\ttarget=cfg(not(target_arch = \"wasm32\"))",
    ];
    for line in &need {
        if !text.contains(line) {
            return Err(format!("missing [{line}] in\n{text}"));
        }
    }
    if text.contains("resolve_node\ttiny-keccak") || text.contains("resolve_node\tother") {
        return Err("absent crates must not be resolve nodes".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let result = match cmd.as_str() {
        "test" => self_test(),
        "scan" => {
            let path = args.next().ok_or_else(|| "scan needs input".to_string());
            let dest = args.next().ok_or_else(|| "scan needs output".to_string());
            path.and_then(|path| {
                dest.and_then(|dest| {
                    let bytes = fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
                    let doc = Parser::new(&bytes).parse()?;
                    let mut file = fs::File::create(&dest).map_err(|e| format!("create {dest}: {e}"))?;
                    write_report(&doc, &mut file)?;
                    Ok(())
                })
            })
        }
        _ => Err("usage: meta_facts test | meta_facts scan <metadata.json> <out>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr(), "meta_facts: {err}");
            ExitCode::from(1)
        }
    }
}
