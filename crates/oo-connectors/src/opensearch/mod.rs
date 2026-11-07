//! OpenSearch/Elasticsearch PIT snapshots. No log CDC.
//! C5 ranges use an explicitly mapped, single-valued keyword field, never PIT
//! `_shard_doc` positions (those are only search_after cursors within one PIT).

use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    SourceError,
    document::{self, Result, ScalarColumn, Stats},
    engine::*,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Product {
    Opensearch,
    Elasticsearch,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub columns: Vec<ScalarColumn>,
    pub range_field: String,
    /// Source keyword-order cuts, fixed across C5 cycles.
    pub boundaries: Vec<String>,
}

pub struct SearchConfig {
    pub product: Product,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub ca_file: Option<PathBuf>,
    pub schema: String,
    pub indices: BTreeMap<String, Index>,
}

struct Snapshot {
    pit: String,
    plan: TableChunkPlan,
}

pub struct SearchEngine {
    client: Client,
    endpoint: String,
    config: SearchConfig,
    password: Zeroizing<String>,
    budget: ReadBudget,
    snapshots: BTreeMap<TableRef, Snapshot>,
    pub stats: Stats,
}

fn exact_index(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "_all"
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn complete(r: &Value) -> Result<()> {
    if r.get("timed_out").and_then(Value::as_bool) != Some(false)
        || r.pointer("/_shards/failed").and_then(Value::as_u64) != Some(0)
        || r.pointer("/_shards/total")
            .and_then(Value::as_u64)
            .is_none()
        || r.pointer("/_shards/total") != r.pointer("/_shards/successful")
        || r.get("terminated_early").and_then(Value::as_bool) == Some(true)
    {
        return Err(SourceError::protocol("search_incomplete_response"));
    }
    Ok(())
}

fn text_bound(bound: Option<&[KeyValue]>) -> Result<Option<&str>> {
    match bound {
        None => Ok(None),
        Some([KeyValue::Text(s)]) => Ok(Some(s)),
        _ => Err(SourceError::config("search_range_bound")),
    }
}

impl SearchEngine {
    pub fn new(
        config: SearchConfig,
        password: Zeroizing<String>,
        budget: ReadBudget,
    ) -> Result<Self> {
        document::validate_budget(&budget)?;
        if config.indices.is_empty()
            || config.indices.len() > 10_000
            || config.schema.is_empty()
            || config.host.contains(['/', '?', '#', '@'])
        {
            return Err(SourceError::config("search_scope"));
        }
        for (name, index) in &config.indices {
            if !exact_index(name)
                || index.range_field.is_empty()
                || index.boundaries.len() > 100_000
                || index.boundaries.windows(2).any(|w| w[0] >= w[1])
            {
                return Err(SourceError::config("search_scope"));
            }
            document::validate_columns(&index.columns)?;
        }
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(1)
            .connect_timeout(Duration::from_millis(budget.statement_timeout_ms));
        let scheme = if let Some(path) = &config.ca_file {
            let pem = std::fs::read(path).map_err(|_| SourceError::tls("search_ca"))?;
            let ca =
                reqwest::Certificate::from_pem(&pem).map_err(|_| SourceError::tls("search_ca"))?;
            builder = builder.tls_certs_only([ca]);
            "https"
        } else if matches!(config.host.as_str(), "127.0.0.1" | "::1") {
            "http"
        } else {
            return Err(SourceError::tls("search_tls_required"));
        };
        let host = if config.host == "::1" {
            "[::1]"
        } else {
            &config.host
        };
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| SourceError::connect("search_client"))?,
            endpoint: format!("{scheme}://{host}:{}", config.port),
            config,
            password,
            budget,
            snapshots: BTreeMap::new(),
            stats: Stats::default(),
        })
    }

    async fn request(&mut self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let started = Instant::now();
        let result = async {
            let mut request = self
                .client
                .request(method, format!("{}{path}", self.endpoint))
                .basic_auth(&self.config.user, Some(self.password.as_str()))
                .timeout(Duration::from_millis(self.budget.statement_timeout_ms));
            if let Some(body) = body {
                request = request
                    .header("Content-Type", "application/json")
                    .body(body.to_string());
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| SourceError::connect("search_request"))?;
            match response.status().as_u16() {
                200..=299 => (),
                401 | 403 => return Err(SourceError::privilege("search_reader_grants")),
                404 => {
                    return Err(SourceError::protocol(
                        "search_rebaseline_required_pit_or_index",
                    ));
                }
                429 => return Err(SourceError::budget("search_throttled")),
                _ => return Err(SourceError::protocol("search_http")),
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| SourceError::protocol("search_body"))?
            {
                if (bytes.len() + chunk.len()) as u64 > self.budget.max_chunk_bytes {
                    return Err(SourceError::budget("search_response_bytes"));
                }
                bytes.extend_from_slice(&chunk);
            }
            self.stats.bytes_received += bytes.len() as u64;
            serde_json::from_slice(&bytes).map_err(|_| SourceError::protocol("search_json"))
        }
        .await;
        self.stats.pace(started, &self.budget).await;
        result
    }

    fn index(&self, table: &TableRef) -> Result<&Index> {
        if table.schema != self.config.schema {
            return Err(SourceError::config("search_scope"));
        }
        self.config
            .indices
            .get(&table.name)
            .ok_or_else(|| SourceError::config("search_scope"))
    }

    pub async fn source_epoch(&mut self) -> Result<String> {
        let root = self.request(Method::GET, "/", None).await?;
        let cluster = root
            .get("cluster_uuid")
            .and_then(Value::as_str)
            .filter(|s| *s != "_na_")
            .ok_or_else(|| SourceError::protocol("search_cluster_uuid"))?;
        let mut identity = vec![cluster.to_owned()];
        for name in self.config.indices.keys().cloned().collect::<Vec<_>>() {
            let r = self.request(Method::GET, &format!("/{name}"), None).await?;
            if r.as_object()
                .is_none_or(|m| m.len() != 1 || !m.contains_key(&name))
            {
                return Err(SourceError::config("search_concrete_index_required"));
            }
            let uuid = r[&name]
                .pointer("/settings/index/uuid")
                .and_then(Value::as_str)
                .ok_or_else(|| SourceError::protocol("search_index_uuid"))?;
            identity.push(format!("{name}:{uuid}:{}", r[&name]["mappings"]));
        }
        identity.push(
            serde_json::to_string(&self.config.indices)
                .map_err(|_| SourceError::config("search_columns"))?,
        );
        Ok(format!(
            "search:v1:{:x}",
            Sha256::digest(identity.join("\n"))
        ))
    }

    async fn search(&mut self, table: &TableRef, mut body: Value) -> Result<Value> {
        let pit = self
            .snapshots
            .get(table)
            .ok_or_else(|| SourceError::config("search_plan_required"))?
            .pit
            .clone();
        body["pit"] = json!({"id": pit, "keep_alive": "5m"});
        body["timeout"] = json!(format!("{}ms", self.budget.statement_timeout_ms));
        let r = self
            .request(
                Method::POST,
                "/_search?allow_partial_search_results=false",
                Some(body),
            )
            .await?;
        // ES can rotate IDs even on an otherwise failed page. Keep the newest
        // ID for cleanup, but never accept partial data.
        if let Some(id) = r.get("pit_id").and_then(Value::as_str) {
            self.snapshots.get_mut(table).unwrap().pit = id.into();
        }
        complete(&r)?;
        Ok(r)
    }

    async fn close_table(&mut self, table: &TableRef) -> Result<()> {
        if let Some(snapshot) = self.snapshots.remove(table) {
            let (path, body) = match self.config.product {
                Product::Opensearch => {
                    ("/_search/point_in_time", json!({"pit_id": [snapshot.pit]}))
                }
                Product::Elasticsearch => ("/_pit", json!({"id": snapshot.pit})),
            };
            self.request(Method::DELETE, path, Some(body)).await?;
        }
        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        let mut result = Ok(());
        for table in self.snapshots.keys().cloned().collect::<Vec<_>>() {
            if let Err(e) = self.close_table(&table).await {
                result = Err(e);
            }
        }
        result
    }

    async fn rows(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<Vec<(String, Value)>> {
        let field = self.index(table)?.range_field.clone();
        let mut range = serde_json::Map::new();
        if let Some(v) = text_bound(lo)? {
            range.insert("gte".into(), json!(v));
        }
        if let Some(v) = text_bound(hi)? {
            range.insert("lt".into(), json!(v));
        }
        let query = if range.is_empty() {
            json!({"match_all": {}})
        } else {
            json!({"range": {field: range}})
        };
        let mut after: Option<Value> = None;
        let mut rows = Vec::new();
        let mut bytes = 0u64;
        loop {
            let mut body = json!({ "size": self.budget.max_chunk_rows.min(1000),
                "sort": ["_shard_doc"], "track_total_hits": false, "query": query });
            if let Some(after) = &after {
                body["search_after"] = after.clone();
            }
            let reply = self.search(table, body).await?;
            let hits = reply
                .pointer("/hits/hits")
                .and_then(Value::as_array)
                .ok_or_else(|| SourceError::protocol("search_hits"))?;
            if hits.is_empty() {
                break;
            }
            for hit in hits {
                let id = hit
                    .get("_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| SourceError::protocol("search_id"))?;
                let doc = hit
                    .get("_source")
                    .filter(|s| s.is_object())
                    .ok_or_else(|| SourceError::protocol("search_source_required"))?;
                bytes += id.len() as u64 + doc.to_string().len() as u64;
                if rows.len() as u64 >= self.budget.max_chunk_rows
                    || bytes > self.budget.max_chunk_bytes
                {
                    return Err(SourceError::budget("search_chunk"));
                }
                rows.push((id.to_owned(), doc.clone()));
            }
            let next = hits
                .last()
                .unwrap()
                .get("sort")
                .filter(|v| v.is_array())
                .ok_or_else(|| SourceError::protocol("search_cursor"))?
                .clone();
            if after.as_ref() == Some(&next) {
                return Err(SourceError::protocol("search_cursor_stalled"));
            }
            after = Some(next);
        }
        Ok(rows)
    }
}

impl SourceEngine for SearchEngine {
    // The shared v1 family enum groups search engines under Opensearch; the
    // product is always explicitly probed and reported, never auto-fallback.
    fn family(&self) -> EngineFamily {
        EngineFamily::Opensearch
    }
    async fn discover_tables(&mut self, _: &ReadBudget) -> Result<Vec<TableRef>> {
        Ok(self
            .config
            .indices
            .keys()
            .map(|name| TableRef {
                schema: self.config.schema.clone(),
                name: name.clone(),
            })
            .collect())
    }
    async fn probe(&mut self) -> Result<ProbeReport> {
        let r = self.request(Method::GET, "/", None).await?;
        let version = EngineVersion::parse(
            self.family(),
            r.pointer("/version/number")
                .and_then(Value::as_str)
                .ok_or_else(|| SourceError::version("search_version"))?,
        )?;
        let os = r.pointer("/version/distribution").and_then(Value::as_str) == Some("opensearch");
        let valid = match self.config.product {
            Product::Opensearch => os && matches!(version.major, 2 | 3),
            Product::Elasticsearch => {
                !os && matches!(version.major, 8 | 9)
                    && r.get("tagline").and_then(Value::as_str) == Some("You Know, for Search")
            }
        };
        if !valid {
            return Err(SourceError::version("search_product_version_mismatch"));
        }
        Ok(ProbeReport {
            protocol: ENGINE_PROTOCOL.into(),
            family: self.family(),
            version,
            capabilities: [
                Capability::Catalog,
                Capability::ChunkedSnapshot,
                Capability::DigestDiff,
            ]
            .into_iter()
            .map(|c| (c, CapabilityState::Available))
            .chain(
                [
                    Capability::LogCdc,
                    Capability::NativeChanges,
                    Capability::Watermark,
                ]
                .into_iter()
                .map(|c| {
                    (
                        c,
                        CapabilityState::Unsupported {
                            reason: "PIT snapshots and C5 only".into(),
                        },
                    )
                }),
            )
            .collect(),
            settings: BTreeMap::from([
                (
                    "product".into(),
                    match self.config.product {
                        Product::Opensearch => "opensearch",
                        Product::Elasticsearch => "elasticsearch",
                    }
                    .into(),
                ),
                ("consistency".into(), "per_index_pit".into()),
                (
                    "digest_execution".into(),
                    "bounded_client_scan_not_server_digest".into(),
                ),
            ]),
        })
    }
    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan> {
        document::validate_budget(budget)?;
        let index = self.index(table)?.clone();
        self.budget = budget.clone();
        self.close_table(table).await?;
        let mapping = self
            .request(Method::GET, &format!("/{}/_mapping", table.name), None)
            .await?;
        let properties = &mapping[&table.name]["mappings"]["properties"];
        let f = &properties[&index.range_field];
        if f.get("type").and_then(Value::as_str) != Some("keyword")
            || f.get("normalizer").is_some()
            || f.get("doc_values") == Some(&Value::Bool(false))
        {
            return Err(SourceError::config(
                "search_range_requires_unnormalized_keyword",
            ));
        }
        let path = match self.config.product {
            Product::Opensearch => format!(
                "/{}/_search/point_in_time?keep_alive=5m&allow_partial_pit_creation=false",
                table.name
            ),
            Product::Elasticsearch => format!(
                "/{}/_pit?keep_alive=5m&allow_partial_search_results=false",
                table.name
            ),
        };
        let r = self.request(Method::POST, &path, None).await?;
        let pit = r
            .get("pit_id")
            .or_else(|| r.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| SourceError::protocol("search_pit"))?
            .to_owned();
        let chunks = (0..=index.boundaries.len())
            .map(|i| ChunkSpec {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: i as u32,
                lo: i
                    .checked_sub(1)
                    .map(|i| vec![KeyValue::Text(index.boundaries[i].clone())]),
                hi: index
                    .boundaries
                    .get(i)
                    .map(|v| vec![KeyValue::Text(v.clone())]),
                estimated_rows: None,
            })
            .collect();
        let plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: KeyShape::NoKey,
            strategy: ChunkStrategy::Keyset {
                columns: vec![index.range_field.clone()],
            },
            estimated_rows: None,
            chunks,
        };
        self.snapshots.insert(
            table.clone(),
            Snapshot {
                pit,
                plan: plan.clone(),
            },
        );
        if r.pointer("/_shards/failed").and_then(Value::as_u64) != Some(0)
            || r.pointer("/_shards/total")
                .and_then(Value::as_u64)
                .is_none()
            || r.pointer("/_shards/total") != r.pointer("/_shards/successful")
        {
            let _ = self.close_table(table).await;
            return Err(SourceError::protocol("search_incomplete_pit"));
        }
        // Multivalued/missing keys would overlap or omit ranges. Check the same
        // PIT that will be read, not a race-prone live count before opening it.
        let checked = self
            .search(
                table,
                json!({"size": 1, "_source": false,
                "query": {"script": {"script": {
                    "source": "doc[params.field].size() != 1",
                    "params": {"field": index.range_field}
                }}}}),
            )
            .await
            .and_then(|r| {
                let hits = r
                    .pointer("/hits/hits")
                    .and_then(Value::as_array)
                    .ok_or_else(|| SourceError::protocol("search_hits"))?;
                if !hits.is_empty() {
                    return Err(SourceError::config("search_range_requires_one_value"));
                }
                Ok(())
            });
        if let Err(e) = checked {
            let _ = self.close_table(table).await;
            return Err(e);
        }
        Ok(plan)
    }
    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt> {
        document::validate_budget(budget)?;
        self.budget = budget.clone();
        if !self
            .snapshots
            .get(&chunk.table)
            .is_some_and(|s| s.plan.chunks.contains(chunk))
        {
            return Err(SourceError::config("search_unknown_chunk"));
        }
        let rows = self
            .rows(&chunk.table, chunk.lo.as_deref(), chunk.hi.as_deref())
            .await?;
        let batch = document::batch(&self.index(&chunk.table)?.columns, &rows)?;
        let (bytes, sha256) = document::ipc(&batch, budget)?;
        sink.push(batch)?;
        Ok(ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows: rows.len() as u64,
            bytes,
            sha256,
            low: None,
            high: None,
        })
    }
    async fn watermark(&mut self) -> Result<Option<Watermark>> {
        Ok(None)
    }
    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        let rows = self.rows(table, lo, hi).await?;
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[_]>::to_vec),
            hi: hi.map(<[_]>::to_vec),
            rows: rows.len() as u64,
            digest: document::digest(&rows),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_partial_search_and_inexact_indices() {
        assert!(
            complete(&json!({"timed_out":false,"_shards":{"failed":0,"total":2,"successful":2}}))
                .is_ok()
        );
        for reply in [
            json!({}),
            json!({"timed_out":true,"_shards":{"failed":0}}),
            json!({"timed_out":false,"_shards":{"failed":1}}),
        ] {
            assert!(complete(&reply).is_err());
        }
        for name in ["*", "_all", "one,two", "../index", "index?pretty"] {
            assert!(!exact_index(name));
        }
        assert!(exact_index("fixture-events"));
    }
    #[test]
    fn bounds_are_typed_and_closed() {
        assert_eq!(text_bound(None).unwrap(), None);
        assert!(text_bound(Some(&[KeyValue::Int(1)])).is_err());
        assert!(text_bound(Some(&[])).is_err());
    }
}
