//! The unified query surface of `oo serve`: `/v1/ontology/*` (D83).
//!
//! One result set over the document ontology (engine, scoped by the Agent token) and the
//! database ontology (Gateway, through the Gateway profile the token names). Every item has a
//! stable URN and a provenance: a file anchor or a table row. Database rows are read only
//! through the Gateway; this crate links no database driver.

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::{Value, json};

use super::proxy::{Head, ProxyConfig, error, response, token_ok};
use crate::lifecycle;

const SEARCH: &str = "oo.ontology.search/v1";
const ENTITY: &str = "oo.ontology.entity/v1";
const SOURCES: &str = "oo.ontology.sources/v1";
const QUERY: &str = "oo.ontology.query/v1";

/// What the caller may do: the owner's control token, or an introspected Agent token.
struct Principal {
    name: String,
    scopes: Vec<String>,
    drive_ids: Vec<String>,
    all_drives: bool,
    gw_profile: Option<String>,
    /// Bearer the engine sees: the Agent's own token (scope checks and audit), or the engine token.
    bearer: String,
    owner: bool,
}

impl Principal {
    fn has(&self, s: &str) -> bool {
        self.owner || self.scopes.iter().any(|x| x == s)
    }
    fn allows_drive(&self, d: &str) -> bool {
        self.all_drives || self.drive_ids.iter().any(|x| x.eq_ignore_ascii_case(d))
    }
}

struct Fail(u16, &'static str, String);

fn fail(status: u16, code: &'static str, msg: impl Into<String>) -> Fail {
    Fail(status, code, msg.into())
}

struct Engine {
    base: String,
    port: u16,
    http: reqwest::Client,
    engine_token: String,
}

impl Engine {
    async fn call(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), Fail> {
        let url = format!("{}{path}", self.base);
        let mut r = if method == "POST" {
            self.http.post(url)
        } else {
            self.http.get(url)
        }
        .bearer_auth(bearer)
        .header("x-oo-public-host", format!("127.0.0.1:{}", self.port));
        if let Some(b) = body {
            r = r
                .header("content-type", "application/json")
                .body(b.to_string());
        }
        let resp = r.send().await.map_err(|_| {
            fail(
                503,
                "DOCS_ENGINE_UNAVAILABLE",
                "the docs engine is not reachable",
            )
        })?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|_| {
            fail(
                503,
                "DOCS_ENGINE_UNAVAILABLE",
                "connection to the engine lost",
            )
        })?;
        Ok((
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        ))
    }

    /// 2xx body, or the engine's refusal mapped through.
    async fn ok(&self, path: &str, bearer: &str) -> Result<Value, Fail> {
        let (s, v) = self.call("GET", path, bearer, None).await?;
        if (200..300).contains(&s) {
            return Ok(v);
        }
        let code: &'static str = match s {
            401 => "ONTOLOGY_TOKEN_INVALID",
            403 => "ONTOLOGY_SCOPE_DENIED",
            404 => "ONTOLOGY_NOT_FOUND",
            _ => "ONTOLOGY_ENGINE_ERROR",
        };
        let m = v["message"]
            .as_str()
            .unwrap_or("the engine refused the request");
        Err(fail(
            if s == 401 || s == 403 || s == 404 {
                s
            } else {
                502
            },
            code,
            m,
        ))
    }
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn pct_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (pct_decode(k), pct_decode(v)),
            None => (pct_decode(p), String::new()),
        })
        .collect()
}

/// A stable name of one thing in the unified namespace.
#[derive(Debug, PartialEq, Eq)]
pub enum Urn {
    Doc {
        drive: String,
        key: String,
    },
    Db {
        source: String,
        table: String,
        pk: String,
    },
}

pub fn doc_urn(drive: &str, key: &str) -> String {
    format!("oo:doc/{drive}/{}", pct_encode(key))
}

pub fn db_urn(source: &str, table: &str, pk: &str) -> String {
    format!("oo:db/{source}/{table}/{}", pct_encode(pk))
}

pub fn parse_urn(s: &str) -> Option<Urn> {
    if let Some(r) = s.strip_prefix("oo:doc/") {
        let (drive, key) = r.split_once('/')?;
        let key = pct_decode(key);
        return (is_uuid(drive) && !key.is_empty()).then(|| Urn::Doc {
            drive: drive.to_ascii_lowercase(),
            key,
        });
    }
    let r = s.strip_prefix("oo:db/")?;
    let mut it = r.splitn(3, '/');
    let (source, table, pk) = (it.next()?, it.next()?, pct_decode(it.next()?));
    (is_ident(source) && is_table(table) && !pk.is_empty()).then(|| Urn::Db {
        source: source.to_owned(),
        table: table.to_owned(),
        pk,
    })
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_table(t: &str) -> bool {
    t.split_once('.')
        .is_some_and(|(a, b)| is_ident(a) && is_ident(b))
}

/// A SQL string literal; control characters are dropped.
pub fn sql_lit(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    format!("'{}'", clean.replace('\'', "''"))
}

/// Statement the proxy lets through to the Gateway: one SELECT or WITH, nothing that writes.
pub fn read_only_sql(sql: &str) -> bool {
    let mut words: Vec<String> = Vec::new();
    let (mut cur, mut quote, mut semi) = (String::new(), false, false);
    for c in sql.trim().chars() {
        if quote {
            quote = c != '\'';
            continue;
        }
        if semi && !c.is_whitespace() {
            return false;
        }
        match c {
            '\'' => {
                quote = true;
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            ';' => semi = true,
            c if c.is_alphanumeric() || c == '_' => cur.push(c.to_ascii_lowercase()),
            _ => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if quote {
        return false;
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    matches!(words.first().map(String::as_str), Some("select" | "with"))
        && !words.iter().any(|w| {
            [
                "insert", "update", "delete", "drop", "alter", "create", "truncate", "grant",
                "revoke", "merge", "call", "copy", "into", "replace", "attach",
            ]
            .contains(&w.as_str())
        })
}

/// Rank of a hit: exact name, prefix, contains.
pub fn score(name: &str, q: &str) -> f64 {
    let (n, q) = (name.to_lowercase(), q.trim().to_lowercase());
    if n == q {
        1.0
    } else if n.starts_with(&q) {
        0.8
    } else if n.contains(&q) {
        0.6
    } else {
        0.4
    }
}

/// File provenance of one engine evidence object.
pub fn file_prov(drive: &str, ev: &Value) -> Value {
    json!({"kind": "file", "drive_id": drive, "path": ev["path"], "file_version": ev["version_id"],
           "block_id": ev["block_id"], "anchor": ev["anchor"]})
}

fn first_evidence(entity: &Value) -> Option<Value> {
    for k in ["facts", "aliases"] {
        for it in entity[k].as_array().into_iter().flatten() {
            if let Some(e) = it["evidence"].as_array().and_then(|a| a.first()) {
                return Some(e.clone());
            }
        }
    }
    None
}

struct Rows {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    query_id: Value,
    truncated: bool,
    publication_id: Value,
    data_vector_sha256: Value,
    sources: Value,
    freshness: Value,
}

impl Rows {
    fn objects(&self) -> Vec<Value> {
        self.rows
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                for (c, v) in self.columns.iter().zip(r) {
                    m.insert(c.clone(), v.clone());
                }
                Value::Object(m)
            })
            .collect()
    }
    fn prov(&self, source: &str) -> Value {
        json!({"source": source, "query_id": self.query_id, "publication_id": self.publication_id,
               "data_vector_sha256": self.data_vector_sha256})
    }
}

fn gateway_err(_: crate::outcome::Failure) -> Fail {
    fail(
        502,
        "ONTOLOGY_GATEWAY_UNAVAILABLE",
        "the Gateway profile is unusable or the Gateway is unreachable",
    )
}

fn load_profile(name: &str) -> Result<Value, Fail> {
    let root = lifecycle::directory().map_err(gateway_err)?;
    if name == "default" {
        let p = lifecycle::saved(&root).map_err(gateway_err)?;
        return if p["gateway"].is_string() {
            Ok(p)
        } else {
            Err(fail(
                502,
                "ONTOLOGY_GATEWAY_UNAVAILABLE",
                "no active Gateway profile",
            ))
        };
    }
    crate::profile::load(&root, name).map_err(|_| {
        fail(
            502,
            "ONTOLOGY_GATEWAY_UNAVAILABLE",
            "the token's Gateway profile does not exist here",
        )
    })
}

async fn gateway(
    profile: &str,
    path: &str,
    body: Option<Value>,
) -> Result<(Value, Vec<(String, String)>), Fail> {
    let p = load_profile(profile)?;
    let http = lifecycle::Http::new(&p, false).map_err(gateway_err)?;
    let token = lifecycle::read_token(&p).map_err(gateway_err)?;
    let (s, v, meta) = http
        .call_meta(path, body, Some(&token), false)
        .await
        .map_err(gateway_err)?;
    if (200..300).contains(&s) && v.get("error").is_none() {
        Ok((v, meta))
    } else {
        let code = v["error"]["code"]
            .as_str()
            .unwrap_or("GATEWAY_REFUSED")
            .to_owned();
        Err(fail(
            if s == 401 || s == 403 { 403 } else { 422 },
            "ONTOLOGY_GATEWAY_REFUSED",
            format!("the Gateway refused the query: {code}"),
        ))
    }
}

async fn gw_query(profile: &str, sql: &str, max_rows: u32) -> Result<Rows, Fail> {
    let (v, meta) = gateway(
        profile,
        "/v1/query",
        Some(json!({"sql": sql, "format": "json", "max_rows": max_rows})),
    )
    .await?;
    let header = |n: &str| {
        meta.iter()
            .find(|(k, _)| k == n)
            .map_or(Value::Null, |(_, v)| json!(v))
    };
    let columns = v["columns"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c["name"].as_str().unwrap_or("").to_owned())
        .collect();
    let rows = v["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| r.as_array().cloned().unwrap_or_default())
        .collect();
    Ok(Rows {
        columns,
        rows,
        query_id: v["query_id"].clone(),
        truncated: v["truncated"] == true,
        publication_id: header("x-oo-publication-id"),
        data_vector_sha256: header("x-oo-data-vector-sha256"),
        sources: v["sources"].clone(),
        freshness: v["freshness"].clone(),
    })
}

struct Ctx<'a> {
    eng: &'a Engine,
    p: &'a Principal,
    profile: Option<String>,
}

#[derive(Clone)]
struct Rule {
    table: String,
    key_col: String,
    name_col: String,
    entity_type: String,
}

impl Ctx<'_> {
    fn src(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }

    fn docs(&self) -> bool {
        self.p.has("docs:read")
    }
    fn db(&self) -> Option<&str> {
        (self.p.has("db:read")).then_some(())?;
        self.profile.as_deref()
    }

    async fn drives(&self) -> Result<Vec<Value>, Fail> {
        let v = if self.p.owner {
            self.eng
                .ok("/v1/docs/drives?limit=200", &self.eng.engine_token)
                .await?
        } else {
            self.eng
                .ok("/v1/docs/agent-tokens/self", &self.p.bearer)
                .await?
        };
        let list = if self.p.owner {
            &v["items"]
        } else {
            &v["drives"]
        };
        Ok(list
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["id"].as_str().is_some_and(|i| self.p.allows_drive(i)))
            .take(16)
            .cloned()
            .collect())
    }

    async fn rules(&self, drives: &[Value]) -> Vec<Rule> {
        let mut out = Vec::new();
        for d in drives {
            let id = d["id"].as_str().unwrap_or("");
            let path = format!("/v1/docs/link-rules?drive_id={id}");
            if let Ok(v) = self.eng.ok(&path, &self.eng.engine_token).await {
                for r in v["items"].as_array().into_iter().flatten() {
                    out.push(Rule {
                        table: r["db_table"].as_str().unwrap_or("").to_owned(),
                        key_col: r["key_col"].as_str().unwrap_or("").to_owned(),
                        name_col: r["name_col"].as_str().unwrap_or("").to_owned(),
                        entity_type: r["entity_type"].as_str().unwrap_or("").to_owned(),
                    });
                }
            }
        }
        out
    }

    async fn links(&self, query: &str) -> Vec<Value> {
        match self
            .eng
            .ok(&format!("/v1/docs/links?{query}"), &self.p.bearer)
            .await
        {
            Ok(v) => v["items"].as_array().cloned().unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    async fn search(&self, q: &str, ty: &str, limit: usize) -> Result<Value, Fail> {
        let drives = if self.docs() || self.db().is_some() {
            self.drives().await?
        } else {
            Vec::new()
        };
        let mut warnings: Vec<String> = Vec::new();
        let mut items: Vec<Value> = Vec::new();
        let (mut docs_n, mut db_n) = (0, 0);
        let docs_status = if self.docs() {
            let mut st = "ok";
            for d in &drives {
                let id = d["id"].as_str().unwrap_or("");
                let mut path = format!(
                    "/v1/docs/entities?q={}&drive_id={id}&limit={limit}",
                    pct_encode(q)
                );
                if !ty.is_empty() {
                    path.push_str(&format!("&type={}", pct_encode(ty)));
                }
                let list = match self.eng.ok(&path, &self.p.bearer).await {
                    Ok(v) => v,
                    Err(f) => {
                        st = "error";
                        warnings.push(format!("documents: {}", f.2));
                        continue;
                    }
                };
                for e in list["items"].as_array().into_iter().flatten() {
                    let key = e["key"].as_str().unwrap_or("");
                    let detail = self
                        .eng
                        .ok(
                            &format!("/v1/docs/entities/{}", pct_encode(key)),
                            &self.p.bearer,
                        )
                        .await;
                    let prov = detail.ok().and_then(|d| {
                        first_evidence(&d)
                            .map(|ev| file_prov(id, &ev))
                            .or_else(|| {
                                d["files"].as_array()?.first().map(|p| {
                                    json!({"kind": "file", "drive_id": id, "path": p, "anchor": null})
                                })
                            })
                    });
                    let Some(prov) = prov else { continue };
                    let name = e["name"].as_str().unwrap_or(key);
                    items.push(
                        json!({"urn": doc_urn(id, key), "type": e["type"], "name": name,
                        "side": "doc", "score": score(name, q), "provenance": prov}),
                    );
                    docs_n += 1;
                }
            }
            st
        } else {
            "skipped"
        };
        let db_status = if let Some(profile) = self.db() {
            let mut st = "ok";
            let rules = self.rules(&drives).await;
            let mut seen: Vec<String> = Vec::new();
            let needle: String = q
                .to_lowercase()
                .chars()
                .filter(|c| !"%_\\".contains(*c) && !c.is_control())
                .collect();
            for r in rules.iter().take(16) {
                if seen.contains(&r.table)
                    || (!ty.is_empty() && !r.entity_type.is_empty() && r.entity_type != ty)
                {
                    continue;
                }
                seen.push(r.table.clone());
                let sql = format!(
                    "SELECT {k}, {n} FROM {t} WHERE LOWER({n}) LIKE {pat} LIMIT {limit}",
                    k = r.key_col,
                    n = r.name_col,
                    t = r.table,
                    pat = sql_lit(&format!("%{needle}%"))
                );
                match gw_query(profile, &sql, limit as u32).await {
                    Ok(rows) => {
                        let prov_base = rows.prov(profile);
                        for row in &rows.rows {
                            let pk = cell(row.first());
                            let name = cell(row.get(1));
                            let mut prov = prov_base.clone();
                            prov["kind"] = json!("table_row");
                            prov["table"] = json!(r.table);
                            prov["pk"] = json!(pk);
                            prov["key_column"] = json!(r.key_col);
                            items.push(json!({"urn": db_urn(profile, &r.table, &pk),
                                "type": if r.entity_type.is_empty() { "Row" } else { r.entity_type.as_str() },
                                "name": name, "side": "db", "score": score(&name, q), "provenance": prov}));
                            db_n += 1;
                        }
                    }
                    Err(f) => {
                        st = "error";
                        warnings.push(format!("database: {}", f.2));
                    }
                }
            }
            st
        } else {
            "skipped"
        };
        items.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .partial_cmp(&a["score"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a["side"].as_str().cmp(&b["side"].as_str()))
                .then_with(|| a["urn"].as_str().cmp(&b["urn"].as_str()))
        });
        items.truncate(limit);
        if self.docs() {
            for it in items.iter_mut().take(5) {
                let l = match parse_urn(it["urn"].as_str().unwrap_or("")) {
                    Some(Urn::Doc { drive, key }) => {
                        self.links(&format!("doc={}&drive_id={drive}", pct_encode(&key)))
                            .await
                    }
                    Some(Urn::Db { table, pk, .. }) => {
                        self.links(&format!("db_table={table}&db_key={}", pct_encode(&pk)))
                            .await
                    }
                    None => Vec::new(),
                };
                it["links"] = Value::Array(
                    l.iter()
                        .map(|x| link_item(x, it["side"] == "db", self.src()))
                        .collect(),
                );
            }
        }
        Ok(json!({"protocol": SEARCH, "query": q, "items": items,
            "sides": {"docs": {"status": docs_status, "count": docs_n}, "db": {"status": db_status, "count": db_n}},
            "warnings": warnings}))
    }

    async fn row(
        &self,
        profile: &str,
        table: &str,
        key_col: &str,
        pk: &str,
    ) -> Result<Option<Value>, Fail> {
        let sql = format!(
            "SELECT * FROM {table} WHERE {key_col} = {} LIMIT 1",
            sql_lit(pk)
        );
        let rows = gw_query(profile, &sql, 1).await?;
        let Some(obj) = rows.objects().into_iter().next() else {
            return Ok(None);
        };
        let mut prov = rows.prov(profile);
        prov["kind"] = json!("table_row");
        prov["table"] = json!(table);
        prov["pk"] = json!(pk);
        prov["key_column"] = json!(key_col);
        Ok(Some(
            json!({"urn": db_urn(profile, table, pk), "columns": obj, "provenance": prov}),
        ))
    }

    async fn entity(&self, urn: &str, key_col_hint: Option<&str>) -> Result<Value, Fail> {
        let parsed = parse_urn(urn).ok_or_else(|| {
            fail(
                400,
                "ONTOLOGY_BAD_URN",
                "expected oo:doc/<drive>/<key> or oo:db/<source>/<schema.table>/<pk>",
            )
        })?;
        match parsed {
            Urn::Doc { drive, key } => {
                if !self.docs() {
                    return Err(fail(
                        403,
                        "ONTOLOGY_SCOPE_DENIED",
                        "the token has no docs:read",
                    ));
                }
                if !self.p.allows_drive(&drive) {
                    return Err(fail(404, "ONTOLOGY_NOT_FOUND", "no such entity"));
                }
                let e = self
                    .eng
                    .ok(
                        &format!("/v1/docs/entities/{}", pct_encode(&key)),
                        &self.p.bearer,
                    )
                    .await?;
                let facts: Vec<Value> = e["facts"].as_array().into_iter().flatten().map(|f| {
                    let ev = f["evidence"].as_array().cloned().unwrap_or_default();
                    json!({"id": f["id"], "predicate": f["predicate"], "value": f["value"], "object": f["object"],
                        "valid_from": f["valid_from"], "status": f["status"],
                        "provenance": ev.first().map_or(Value::Null, |x| file_prov(&drive, x)),
                        "evidence": ev.iter().map(|x| file_prov(&drive, x)).collect::<Vec<_>>()})
                }).collect();
                let raw = self
                    .links(&format!("doc={}&drive_id={drive}", pct_encode(&key)))
                    .await;
                let rules = if self.db().is_some() {
                    self.rules(&[json!({"id": drive})]).await
                } else {
                    Vec::new()
                };
                let mut links = Vec::new();
                for l in &raw {
                    let mut it = link_item(l, false, self.src());
                    if let Some(profile) = self.db()
                        && let (Some(t), Some(k)) = (l["db_table"].as_str(), l["db_key"].as_str())
                        && let Some(r) = rules.iter().find(|r| r.table == t)
                        && let Ok(Some(row)) = self.row(profile, t, &r.key_col, k).await
                    {
                        it["row"] = row;
                    }
                    links.push(it);
                }
                Ok(
                    json!({"protocol": ENTITY, "urn": urn, "side": "doc", "type": e["type"], "name": e["name"],
                    "aliases": e["aliases"], "facts": facts, "links": links}),
                )
            }
            Urn::Db { table, pk, .. } => {
                let Some(profile) = self.db() else {
                    return Err(fail(
                        403,
                        "ONTOLOGY_SCOPE_DENIED",
                        "the token has no db:read or names no Gateway profile",
                    ));
                };
                let drives = self.drives().await?;
                let rules = self.rules(&drives).await;
                let key_col = rules
                    .iter()
                    .find(|r| r.table == table)
                    .map(|r| r.key_col.clone())
                    .or_else(|| key_col_hint.filter(|k| is_ident(k)).map(str::to_owned))
                    .ok_or_else(|| {
                        fail(
                            404,
                            "ONTOLOGY_NOT_FOUND",
                            "no link rule names the key column of this table; pass key_col",
                        )
                    })?;
                let row = self
                    .row(profile, &table, &key_col, &pk)
                    .await?
                    .ok_or_else(|| fail(404, "ONTOLOGY_NOT_FOUND", "no such row"))?;
                let links: Vec<Value> = if self.docs() {
                    self.links(&format!("db_table={table}&db_key={}", pct_encode(&pk)))
                        .await
                        .iter()
                        .map(|l| link_item(l, true, self.src()))
                        .collect()
                } else {
                    Vec::new()
                };
                Ok(
                    json!({"protocol": ENTITY, "urn": urn, "side": "db", "type": "Row", "name": Value::Null,
                    "row": row["columns"], "provenance": row["provenance"], "links": links}),
                )
            }
        }
    }

    async fn sources(&self) -> Result<Value, Fail> {
        let mut items = Vec::new();
        if self.docs() {
            for d in self.drives().await? {
                items.push(json!({"kind": "drive", "id": d["id"], "name": d["name"], "backend": d["backend"], "status": d["status"]}));
            }
        }
        if let Some(profile) = self.db() {
            match gateway(profile, "/v1/freshness", None).await {
                Ok((v, _)) => items.push(json!({"kind": "database", "profile": profile, "status": "ok", "freshness": v})),
                Err(f) => items.push(json!({"kind": "database", "profile": profile, "status": "error", "message": f.2})),
            }
        }
        Ok(json!({"protocol": SOURCES, "items": items}))
    }

    async fn query(&self, sql: &str, max_rows: u32) -> Result<Value, Fail> {
        let Some(profile) = self.db() else {
            return Err(fail(
                403,
                "ONTOLOGY_SCOPE_DENIED",
                "the token has no db:read or names no Gateway profile",
            ));
        };
        if !read_only_sql(sql) {
            return Err(fail(
                403,
                "ONTOLOGY_READ_ONLY",
                "only one read-only SELECT is accepted; the ontology is not writable through this surface",
            ));
        }
        let rows = gw_query(profile, sql, max_rows).await?;
        let mut prov = rows.prov(profile);
        prov["kind"] = json!("gateway_query");
        prov["sources"] = rows.sources.clone();
        prov["freshness"] = rows.freshness.clone();
        Ok(
            json!({"protocol": QUERY, "columns": rows.columns, "rows": rows.rows,
            "row_count": rows.rows.len(), "truncated": rows.truncated, "provenance": prov}),
        )
    }
}

fn cell(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

/// One engine link as the other end sees it: `to_db` is false when seen from the document.
fn link_item(l: &Value, from_db: bool, source: &str) -> Value {
    let drive = l["drive_id"].as_str().unwrap_or("");
    let target = if from_db {
        json!({"urn": doc_urn(drive, l["doc_entity_key"].as_str().unwrap_or("")), "name": l["canonical_name"], "type": l["doc_entity_type"]})
    } else {
        json!({"urn": db_urn(source, l["db_table"].as_str().unwrap_or(""), l["db_key"].as_str().unwrap_or(""))})
    };
    json!({"target": target, "db_table": l["db_table"], "db_key": l["db_key"], "status": l["status"],
        "confidence_bp": l["confidence_bp"], "matched_via": l["matched_via"], "inferred": l["status"] == "auto",
        "provenance": l["provenance"]})
}

fn reply(r: Result<Value, Fail>) -> Vec<u8> {
    match r {
        Ok(v) => response(200, &v.to_string(), &[]),
        Err(Fail(s, c, m)) => error(s, c, &m, s >= 500, &[]),
    }
}

/// Answer one `/v1/ontology/*` request; the proxy has checked Origin and Host.
pub async fn serve(head: &Head, body: &[u8], cfg: &ProxyConfig) -> Vec<u8> {
    let Some(addr) = cfg.engine.get() else {
        return error(
            503,
            "DOCS_ENGINE_UNAVAILABLE",
            "the docs engine is not reachable",
            true,
            &[("retry-after", "1")],
        );
    };
    reply(run(head, body, cfg, addr).await)
}

async fn run(head: &Head, body: &[u8], cfg: &ProxyConfig, addr: SocketAddr) -> Result<Value, Fail> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| fail(500, "ONTOLOGY_INTERNAL", "http client"))?;
    let eng = Engine {
        base: format!("http://{addr}"),
        port: cfg.port,
        http,
        engine_token: cfg.engine_token.clone(),
    };
    let auth = head.header("authorization");
    let bearer = auth
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    let query = parse_query(head.target.split_once('?').map_or("", |(_, q)| q));
    let qv = |k: &str| query.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    let principal = if token_ok(auth, &cfg.token_sha256) {
        Principal {
            name: "owner".into(),
            scopes: vec![],
            drive_ids: vec![],
            all_drives: true,
            gw_profile: qv("profile").map(str::to_owned),
            bearer: cfg.engine_token.clone(),
            owner: true,
        }
    } else if bearer.starts_with("ooa_") {
        let (s, v) = eng
            .call(
                "POST",
                "/v1/docs/agent-tokens:introspect",
                &cfg.engine_token,
                Some(json!({"token": bearer})),
            )
            .await?;
        if s != 200 || v["active"] != true {
            return Err(fail(
                401,
                "ONTOLOGY_TOKEN_INVALID",
                "the Agent token is invalid, expired or revoked",
            ));
        }
        let strs = |k: &str| {
            v[k].as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        Principal {
            name: v["name"].as_str().unwrap_or("agent").to_owned(),
            scopes: strs("scopes"),
            drive_ids: strs("drive_ids"),
            all_drives: v["all_drives"] == true,
            gw_profile: v["gw_profile"].as_str().map(str::to_owned),
            bearer: bearer.to_owned(),
            owner: false,
        }
    } else {
        return Err(fail(
            401,
            "DOCS_ENGINE_UNAUTHENTICATED",
            "a valid bearer token is required",
        ));
    };
    let _ = &principal.name;
    let profile = principal
        .gw_profile
        .clone()
        .or_else(|| principal.owner.then(|| "default".to_owned()));
    let profile = if principal.owner {
        qv("profile").map(str::to_owned).or(profile)
    } else {
        profile
    };
    let cx = Ctx {
        eng: &eng,
        p: &principal,
        profile,
    };
    let path = head.path();
    let limit = qv("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 50);
    match (
        head.method.as_str(),
        path.trim_start_matches("/v1/ontology/"),
    ) {
        ("GET", "search") => {
            let q = qv("q")
                .map(str::trim)
                .filter(|q| !q.is_empty() && q.len() <= 200)
                .ok_or_else(|| {
                    fail(
                        400,
                        "ONTOLOGY_BAD_REQUEST",
                        "q is required (1..=200 characters)",
                    )
                })?;
            cx.search(q, qv("type").unwrap_or(""), limit).await
        }
        ("GET", "sources") => cx.sources().await,
        ("GET", rest) if rest.starts_with("entities/") => {
            cx.entity(&pct_decode(&rest["entities/".len()..]), qv("key_col"))
                .await
        }
        ("GET", "entity") => {
            let u =
                qv("urn").ok_or_else(|| fail(400, "ONTOLOGY_BAD_REQUEST", "urn is required"))?;
            cx.entity(u, qv("key_col")).await
        }
        ("POST", "query") => {
            let b: Value = serde_json::from_slice(body)
                .map_err(|_| fail(400, "ONTOLOGY_BAD_REQUEST", "expected a JSON object"))?;
            let sql = b["sql"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 16384)
                .ok_or_else(|| fail(400, "ONTOLOGY_BAD_REQUEST", "sql is required"))?;
            let n = b["max_rows"].as_u64().map_or(200, |n| n.clamp(1, 2000)) as u32;
            cx.query(sql, n).await
        }
        _ => Err(fail(404, "ONTOLOGY_NOT_FOUND", "unknown ontology route")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urns_round_trip() {
        let d = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let u = doc_urn(d, "org:acme/ltd");
        assert_eq!(
            parse_urn(&u),
            Some(Urn::Doc {
                drive: d.into(),
                key: "org:acme/ltd".into()
            })
        );
        let u = db_urn("sales", "sales.customers", "a/b 1");
        assert_eq!(
            parse_urn(&u),
            Some(Urn::Db {
                source: "sales".into(),
                table: "sales.customers".into(),
                pk: "a/b 1".into()
            })
        );
        for bad in [
            "oo:doc/x/y",
            "oo:db/a/b/c",
            "oo:db/a/s.t/",
            "http://x",
            "oo:doc/",
        ] {
            assert!(parse_urn(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn only_one_read_statement_passes() {
        assert!(read_only_sql(
            "SELECT credit_limit FROM sales.customers WHERE name='Acme; Ltd'"
        ));
        assert!(read_only_sql("with a as (select 1) select * from a;"));
        for bad in [
            "DELETE FROM t",
            "SELECT 1; DROP TABLE t",
            "select * into x from t",
            "update t set a=1",
            "",
            "select 'x",
        ] {
            assert!(!read_only_sql(bad), "{bad}");
        }
    }

    #[test]
    fn literals_and_scores() {
        assert_eq!(sql_lit("O'Neil"), "'O''Neil'");
        assert!(score("Acme Ltd", "acme ltd") > score("Acme Ltd", "acme"));
        assert!(score("Acme Ltd", "acme") > score("The Acme Ltd", "acme"));
    }

    #[test]
    fn queries_decode() {
        assert_eq!(
            parse_query("q=Acme%20Ltd&limit=5"),
            vec![
                ("q".into(), "Acme Ltd".into()),
                ("limit".into(), "5".into())
            ]
        );
    }

    async fn fake_engine() -> SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_owned();
                let d = "0f8fad5b-d9cb-469f-a165-70867728950e";
                let (status, body) = if line.contains("agent-tokens:introspect") {
                    if req.contains("ooa_revoked") {
                        (200, json!({"active": false, "reason": "revoked"}))
                    } else {
                        (
                            200,
                            json!({"active": true, "name": "bot", "scopes": ["docs:read"], "drive_ids": [d], "all_drives": false, "gw_profile": null}),
                        )
                    }
                } else if line.contains("agent-tokens/self") {
                    (
                        200,
                        json!({"drives": [{"id": d, "name": "default", "backend": "fs", "status": "ready"}]}),
                    )
                } else if line.contains("/v1/docs/entities?") {
                    (
                        200,
                        json!({"items": [{"key": "org:acme", "type": "Organization", "name": "Acme Ltd"}]}),
                    )
                } else if line.contains("/v1/docs/entities/") {
                    (
                        200,
                        json!({"facts": [{"id": "f1", "predicate": "attr.note", "value": "x",
                        "evidence": [{"path": "notes/acme.md", "version_id": "v1", "block_id": "b1", "anchor": {"heading": "Acme"}}]}], "aliases": []}),
                    )
                } else if line.contains("/v1/docs/links?") {
                    (200, json!({"items": []}))
                } else {
                    (404, json!({"message": "no"}))
                };
                let b = body.to_string();
                let _ = s.write_all(format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len()).as_bytes()).await;
            }
        });
        addr
    }

    async fn call(addr: SocketAddr, token: &str, target: &str) -> (u16, Value) {
        use sha2::{Digest, Sha256};
        let link = std::sync::Arc::new(super::super::proxy::EngineLink::default());
        link.set(Some(addr));
        let cfg = ProxyConfig {
            port: 1,
            token_sha256: Sha256::digest(b"oodt_x").into(),
            engine_token: "oodi".into(),
            engine: link,
            drives: None,
        };
        let head = Head {
            method: "GET".into(),
            target: target.into(),
            headers: vec![("authorization".into(), format!("Bearer {token}"))],
            rest: vec![],
        };
        let out = String::from_utf8(serve(&head, b"", &cfg).await).unwrap();
        let status = out[9..12].parse().unwrap();
        (
            status,
            serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
        )
    }

    #[tokio::test]
    async fn ontology_federate_docs_only_degrades_and_cites() {
        let addr = fake_engine().await;
        let (s, v) = call(addr, "ooa_good", "/v1/ontology/search?q=acme").await;
        assert_eq!(s, 200, "{v}");
        let it = &v["items"][0];
        assert_eq!(it["side"], "doc");
        assert_eq!(
            it["urn"],
            "oo:doc/0f8fad5b-d9cb-469f-a165-70867728950e/org:acme"
        );
        assert_eq!(it["provenance"]["path"], "notes/acme.md");
        assert_eq!(v["sides"]["db"]["status"], "skipped");
        // no db:read: a query is refused, a write even before that
        let (s, v) = call(addr, "ooa_good", "/v1/ontology/sources").await;
        assert_eq!((s, v["items"][0]["kind"].as_str()), (200, Some("drive")));
    }

    #[tokio::test]
    async fn ontology_federate_revoked_token_is_401() {
        let addr = fake_engine().await;
        let (s, v) = call(addr, "ooa_revoked", "/v1/ontology/search?q=acme").await;
        assert_eq!(
            (s, v["code"].as_str()),
            (401, Some("ONTOLOGY_TOKEN_INVALID"))
        );
        let (s, _) = call(addr, "nonsense", "/v1/ontology/search?q=acme").await;
        assert_eq!(s, 401);
    }

    #[test]
    fn links_cite_both_ends() {
        let l = json!({"drive_id":"d","doc_entity_key":"org:a","canonical_name":"A","db_table":"s.t","db_key":"1","status":"auto","provenance":{"file":[]}});
        assert_eq!(link_item(&l, true, "s")["target"]["urn"], "oo:doc/d/org:a");
        assert_eq!(link_item(&l, false, "s")["inferred"], true);
    }
}
