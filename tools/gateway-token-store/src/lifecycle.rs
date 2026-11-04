//! v2 lifecycle, compiled by the Gateway alongside (not instead of) pinned v1.
//! All mutation and audit writes share one transaction and the v1 project lock.
use crate::accepted_token_store::{self as legacy, Endpoint, TokenKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_postgres::{Client, Transaction};
use zeroize::{Zeroize, Zeroizing};

pub const DAY: i64 = 86_400_000;
const SQL: &str = include_str!("../migrations/oo_token_control_v2.sql");
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub max_lifetime_ms: i64,
    pub default_ttl_ms: i64,
    pub self_renew_enabled: bool,
    pub self_renew_window_ms: i64,
    pub rotate_overlap_ms: i64,
    pub enroll_ttl_ms: i64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_lifetime_ms: 90 * DAY,
            default_ttl_ms: 90 * DAY,
            self_renew_enabled: false,
            self_renew_window_ms: 14 * DAY,
            rotate_overlap_ms: 7 * DAY,
            enroll_ttl_ms: DAY,
        }
    }
}
impl Policy {
    fn validate(&self) -> Result<()> {
        if !(1..=365 * DAY).contains(&self.max_lifetime_ms)
            || !(1..=self.max_lifetime_ms).contains(&self.default_ttl_ms)
            || !(0..=self.max_lifetime_ms).contains(&self.self_renew_window_ms)
            || !(0..=30 * DAY).contains(&self.rotate_overlap_ms)
            || !(1..=7 * DAY).contains(&self.enroll_ttl_ms)
        {
            return Err("policy");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub project_id: String,
    pub principal_id: String,
    pub role: String,
    #[serde(default)]
    pub claims: BTreeMap<String, String>,
    pub policy_version: String,
    pub knowledge_version: String,
    pub audience_url: String,
    #[serde(default)]
    pub ttl_ms: Option<i64>,
    #[serde(default)]
    pub purpose: String,
}
fn ident(s: &str, max: usize) -> Result<()> {
    if s.is_empty()
        || s.len() > max
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
    {
        return Err("input");
    }
    Ok(())
}
fn safe_text(s: &str) -> Result<()> {
    if s.len() > 256
        || s.chars().any(char::is_control)
        || ["ooct_", "oost_", "OO-"].iter().any(|p| s.contains(p))
    {
        return Err("input");
    }
    Ok(())
}
fn audience(s: &str) -> Result<String> {
    let uri: axum::http::Uri = s.parse().map_err(|_| "audience")?;
    if uri.scheme_str() != Some("https")
        || uri.host().is_none()
        || s.contains(['@', '?', '#'])
        || s.len() > 256
        || uri.path().contains("//")
        || uri.path().split('/').any(|s| s == "." || s == "..")
    {
        return Err("audience");
    }
    let host = uri.host().ok_or("audience")?.to_ascii_lowercase();
    // Keep issued tuples readable by the accepted v1 audience validator.
    if host.starts_with('[')
        || host.ends_with('.')
        || host.split('.').any(str::is_empty)
        || (uri.authority().ok_or("audience")?.as_str().contains(':') && uri.port_u16().is_none())
    {
        return Err("audience");
    }
    let port = uri
        .port_u16()
        .filter(|p| *p != 443)
        .map(|p| format!(":{p}"))
        .unwrap_or_default();
    Ok(format!(
        "https://{host}{port}{}",
        if uri.path().is_empty() {
            "/"
        } else {
            uri.path()
        }
    ))
}
impl Grant {
    pub fn from_legacy(grant: legacy::IssueGrant, purpose: String) -> Result<Self> {
        if grant.kind != TokenKind::Client
            || grant.current.sandbox_id.is_some()
            || grant.current.session_id.is_some()
        {
            return Err("binding");
        }
        let count = grant.current.claims.len();
        let mut result = Self {
            project_id: grant.current.project_id,
            principal_id: grant.current.principal_id,
            role: grant.current.role,
            claims: grant.current.claims.into_iter().collect(),
            policy_version: grant.current.policy_version,
            knowledge_version: grant.current.knowledge_version,
            audience_url: grant.current.audience_url,
            ttl_ms: grant.client_ttl_ms,
            purpose,
        };
        if result.claims.len() != count {
            return Err("claims");
        }
        result.validate()?;
        Ok(result)
    }

    fn validate(&mut self) -> Result<()> {
        for s in [&self.project_id, &self.principal_id, &self.role] {
            ident(s, 128)?;
        }
        for s in [&self.policy_version, &self.knowledge_version] {
            ident(s, 64)?;
        }
        if ["oo_token_admin", "oo_token_runtime", "oo_fixture_admin"]
            .contains(&self.principal_id.as_str())
            || [
                "public",
                "admin",
                "oo_token_admin",
                "oo_token_runtime",
                "oo_fixture_admin",
            ]
            .contains(&self.role.as_str())
        {
            return Err("principal_or_role");
        }
        if self.claims.len() > 32 {
            return Err("claims");
        }
        for (k, v) in &self.claims {
            ident(k, 64)?;
            if [
                "allow",
                "auth",
                "authenticated",
                "authentication",
                "caller_authenticated",
                "groups",
                "idp",
                "mtls",
                "role",
                "roles",
                "self_claim",
            ]
            .contains(&k.as_str())
                || v.chars().any(char::is_control)
            {
                return Err("claims");
            }
        }
        if serde_json::to_string(&self.claims)
            .map_err(|_| "claims")?
            .len()
            > 2048
        {
            return Err("claims");
        }
        safe_text(&self.purpose)?;
        self.audience_url = audience(&self.audience_url)?;
        Ok(())
    }
}

// Requests containing enrollment material deliberately do not implement Debug.
pub enum Operation {
    Issue(Grant),
    List {
        project: String,
    },
    Renew {
        project: String,
        hash: String,
        deadline_ms: i64,
    },
    SelfRenew {
        token: Zeroizing<String>,
        audience: String,
    },
    Rotate {
        project: String,
        hash: String,
    },
    Revoke {
        project: String,
        hash: String,
    },
    Enroll(Grant),
    Redeem {
        code: Zeroizing<String>,
        audience: String,
    },
    Policy {
        project: String,
        policy: Policy,
    },
}
impl Operation {
    fn name(&self) -> &'static str {
        match self {
            Self::Issue(_) => "issue",
            Self::List { .. } => "list",
            Self::Renew { .. } => "renew",
            Self::SelfRenew { .. } => "self_renew",
            Self::Rotate { .. } => "rotate",
            Self::Revoke { .. } => "revoke",
            Self::Enroll(_) => "enroll",
            Self::Redeem { .. } => "redeem",
            Self::Policy { .. } => "policy",
        }
    }
}
pub struct Receipt {
    pub metadata: Value,
    pub secret: Option<Zeroizing<String>>,
}
impl std::fmt::Debug for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Receipt(redacted)")
    }
}
impl Receipt {
    fn metadata(value: Value) -> Self {
        Self {
            metadata: value,
            secret: None,
        }
    }
}
pub struct Control {
    #[cfg(test)]
    pub(crate) connections: std::sync::atomic::AtomicUsize,
    endpoint: Endpoint,
    password: Zeroizing<String>,
    ca: Vec<u8>,
}
impl Control {
    pub fn new(endpoint: Endpoint, password: String, ca: Vec<u8>) -> Self {
        Self {
            #[cfg(test)]
            connections: std::sync::atomic::AtomicUsize::new(0),
            endpoint,
            password: Zeroizing::new(password),
            ca,
        }
    }
    async fn connect(&self) -> Result<Client> {
        #[cfg(test)]
        self.connections
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&self.endpoint.host)
            .port(self.endpoint.port)
            .dbname(&self.endpoint.database)
            .user("oo_token_admin")
            .password(self.password.as_str())
            .ssl_mode(tokio_postgres::config::SslMode::Require)
            .connect_timeout(Duration::from_secs(10));
        let connector = legacy::rustls_connector(&self.ca).map_err(|_| "tls")?;
        let (client, conn) = cfg.connect(connector).await.map_err(|_| "connect")?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let row = client
            .query_one(
                "SELECT current_user, ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",
                &[],
            )
            .await
            .map_err(|_| "tls")?;
        if row.get::<_, String>(0) != "oo_token_admin" || !row.get::<_, bool>(1) {
            return Err("login_role");
        }
        Ok(client)
    }
    pub async fn install(&self) -> Result<()> {
        legacy::TrustedControlAdapter::new(
            self.endpoint.clone(),
            "oo_token_admin",
            self.password.as_str(),
            self.ca.clone(),
        )
        .map_err(|_| "input")?
        .install()
        .await
        .map_err(|_| "v1_install")?;
        let mut client = self.connect().await?;
        let tx = client.transaction().await.map_err(|_| "database")?;
        tx.batch_execute(
            "SET LOCAL search_path TO pg_catalog; SELECT pg_advisory_xact_lock(881275033)",
        )
        .await
        .map_err(|_| "database")?;
        let exists: bool = tx
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname='oo_token_lifecycle')",
                &[],
            )
            .await
            .map_err(|_| "database")?
            .get(0);
        let hash = digest(SQL.as_bytes());
        if exists {
            verify(&tx).await?;
        } else {
            tx.batch_execute(SQL).await.map_err(|_| "migration")?;
            tx.execute(
                "INSERT INTO oo_token_lifecycle.migration VALUES (2,$1)",
                &[&hash],
            )
            .await
            .map_err(|_| "database")?;
        }
        audit(
            &tx,
            now_ms()?,
            "",
            "oo_token_admin",
            "install",
            "ok",
            &json!({}),
        )
        .await?;
        tx.commit().await.map_err(|_| "commit_unknown")
    }
    pub async fn execute(&self, operation: Operation, actor: &str) -> Result<Receipt> {
        self.execute_at(operation, actor, now_ms()?).await
    }
    /// Injected wall-clock seam; not evidence of the production clock.
    pub async fn execute_at(&self, operation: Operation, actor: &str, now: i64) -> Result<Receipt> {
        safe_text(actor)?;
        if actor.is_empty() || now < 0 {
            return Err("input");
        }
        let mut client = self.connect().await?;
        let tx = client.transaction().await.map_err(|_| "database")?;
        tx.batch_execute("SET LOCAL search_path TO pg_catalog; SET LOCAL lock_timeout='10s'; SET LOCAL statement_timeout='15s'; SET LOCAL synchronous_commit=on").await.map_err(|_|"database")?;
        verify(&tx).await?;
        let project = project_for(&tx, &operation).await?;
        let bytes = Sha256::digest(project.as_bytes());
        let key = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        tx.execute(
            "SELECT pg_advisory_xact_lock(881275033::int,$1::int)",
            &[&key],
        )
        .await
        .map_err(|_| "database")?;
        tx.batch_execute("SAVEPOINT operation")
            .await
            .map_err(|_| "database")?;
        let name = operation.name();
        let result = operate(&tx, operation, actor, now, &project).await;
        if result.is_err() {
            tx.batch_execute("ROLLBACK TO SAVEPOINT operation")
                .await
                .map_err(|_| "database")?;
        }
        let empty = json!({});
        audit(
            &tx,
            now,
            &project,
            actor,
            name,
            result.as_ref().err().copied().unwrap_or("ok"),
            result.as_ref().map(|r| &r.metadata).unwrap_or(&empty),
        )
        .await?;
        tx.commit().await.map_err(|_| "commit_unknown")?;
        result
    }
}
pub fn now_ms() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock")?
            .as_millis(),
    )
    .map_err(|_| "clock")
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
async fn verify(tx: &Transaction<'_>) -> Result<()> {
    let row = tx
        .query_opt(
            "SELECT sql_sha256 FROM oo_token_lifecycle.migration WHERE version=2",
            &[],
        )
        .await
        .map_err(|_| "install_drift")?
        .ok_or("install_drift")?;
    if row.get::<_, String>(0) != digest(SQL.as_bytes()) {
        return Err("install_drift");
    }
    Ok(())
}
async fn audit(
    tx: &Transaction<'_>,
    now: i64,
    project: &str,
    actor: &str,
    op: &str,
    outcome: &str,
    m: &Value,
) -> Result<()> {
    let hash = m["token_sha256"].as_str();
    let related = m["rotated_from"].as_str();
    let old = m["old_deadline_ms"].as_i64();
    let new = m["expires_at_ms"].as_i64();
    tx.execute("INSERT INTO oo_token_lifecycle.audit (at_ms,project_id,actor,operation,outcome,token_sha256,related_sha256,old_deadline_ms,new_deadline_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",&[&now,&project,&actor,&op,&outcome,&hash,&related,&old,&new]).await.map_err(|_|"audit")?;
    Ok(())
}
async fn project_for(tx: &Transaction<'_>, op: &Operation) -> Result<String> {
    Ok(match op {
        Operation::Issue(g) | Operation::Enroll(g) => g.project_id.clone(),
        Operation::List { project }
        | Operation::Renew { project, .. }
        | Operation::Rotate { project, .. }
        | Operation::Revoke { project, .. }
        | Operation::Policy { project, .. } => project.clone(),
        Operation::SelfRenew { token, .. } => {
            let hash = legacy::token_hash(token)
                .map(|(_, h)| h)
                .unwrap_or_default();
            tx.query_opt(
                "SELECT project_id FROM oo_token_control.token_record WHERE token_sha256=$1",
                &[&hash],
            )
            .await
            .map_err(|_| "database")?
            .map(|r| r.get(0))
            .unwrap_or_default()
        }
        Operation::Redeem { code, .. } => tx
            .query_opt(
                "SELECT project_id FROM oo_token_lifecycle.enrollment WHERE code_sha256=$1",
                &[&digest(code.as_bytes())],
            )
            .await
            .map_err(|_| "database")?
            .map(|r| r.get(0))
            .unwrap_or_default(),
    })
}
async fn policy(tx: &Transaction<'_>, project: &str) -> Result<Policy> {
    let row=tx.query_opt("SELECT max_lifetime_ms,default_ttl_ms,self_renew_enabled,self_renew_window_ms,rotate_overlap_ms,enroll_ttl_ms FROM oo_token_lifecycle.policy WHERE project_id=$1",&[&project]).await.map_err(|_|"database")?;
    Ok(match row {
        None => Policy::default(),
        Some(r) => Policy {
            max_lifetime_ms: r.get(0),
            default_ttl_ms: r.get(1),
            self_renew_enabled: r.get(2),
            self_renew_window_ms: r.get(3),
            rotate_overlap_ms: r.get(4),
            enroll_ttl_ms: r.get(5),
        },
    })
}
async fn issue(
    tx: &Transaction<'_>,
    mut g: Grant,
    actor: &str,
    now: i64,
    p: &Policy,
    from: Option<&str>,
    first: Option<i64>,
) -> Result<Receipt> {
    g.validate()?;
    let ttl = g.ttl_ms.unwrap_or(p.default_ttl_ms);
    if !(1..=p.max_lifetime_ms).contains(&ttl) {
        return Err("max_lifetime");
    }
    let deadline = now.checked_add(ttl).ok_or("max_lifetime")?;
    let first = first.unwrap_or(now);
    if deadline > first.checked_add(p.max_lifetime_ms).ok_or("max_lifetime")? {
        return Err("max_lifetime");
    }
    let mut raw = legacy::draw_32().map_err(|_| "randomness")?;
    let token = legacy::PlainToken::issue(TokenKind::Client, &raw);
    raw.zeroize();
    let hash = token.sha256_hex().map_err(|_| "hash")?;
    let claims = serde_json::to_string(&g.claims).map_err(|_| "claims")?;
    tx.execute("INSERT INTO oo_token_control.token_record (token_sha256,token_kind,project_id,principal_id,role_name,claims_canonical,policy_version,knowledge_version,audience_url,issued_at_ms,absolute_deadline_ms,last_used_ms,revocation_floor) VALUES ($1,'client',$2,$3,$4,$5,$6,$7,$8,$9,$10,$9,(SELECT COALESCE(MAX(seq),0) FROM oo_token_control.revocation WHERE project_id=$2 AND (scope='project' OR (scope='principal' AND principal_id=$3))))",&[&hash,&g.project_id,&g.principal_id,&g.role,&claims,&g.policy_version,&g.knowledge_version,&g.audience_url,&now,&deadline]).await.map_err(|_|"database")?;
    tx.execute("INSERT INTO oo_token_lifecycle.token (token_sha256,first_issued_at_ms,rotated_from,issued_by,purpose) VALUES ($1,$2,$3,$4,$5)",&[&hash,&first,&from,&actor,&g.purpose]).await.map_err(|_|"database")?;
    Ok(Receipt {
        metadata: json!({"token_sha256":hash,"first_issued_at_ms":first,"expires_at_ms":deadline,"role":g.role,"audience":g.audience_url,"rotated_from":from}),
        secret: Some(Zeroizing::new(token.reveal_once())),
    })
}
const LIVE: &str = "SELECT t.*, COALESCE(x.first_issued_at_ms,t.issued_at_ms) AS first_ms,x.overlap_until_ms FROM oo_token_control.token_record t LEFT JOIN oo_token_lifecycle.token x USING(token_sha256) WHERE t.project_id=$1 AND t.token_sha256=$2";
async fn live(
    tx: &Transaction<'_>,
    project: &str,
    hash: &str,
    now: i64,
) -> Result<tokio_postgres::Row> {
    let r = tx
        .query_opt(LIVE, &[&project, &hash])
        .await
        .map_err(|_| "database")?
        .ok_or("not_found")?;
    if r.get::<_, String>("token_kind") != "client" {
        return Err("binding");
    }
    let principal: String = r.get("principal_id");
    let floor: i64 = r.get("revocation_floor");
    let revoked=tx.query_opt("SELECT 1 FROM oo_token_control.revocation WHERE project_id=$1 AND ((scope='token' AND token_sha256=$2) OR (seq>$3 AND (scope='project' OR (scope='principal' AND principal_id=$4)))) LIMIT 1",&[&project,&hash,&floor,&principal]).await.map_err(|_|"database")?.is_some();
    if revoked {
        return Err("revoked");
    }
    if now < r.get::<_, i64>("issued_at_ms") || now >= r.get::<_, i64>("absolute_deadline_ms") {
        return Err("expired");
    }
    Ok(r)
}
fn unique_hash(mut hashes: impl Iterator<Item = String>) -> Result<String> {
    let first = hashes.next().ok_or("not_found")?;
    if hashes.next().is_some() {
        return Err("ambiguous_hash_prefix");
    }
    Ok(first)
}
async fn resolve_hash(tx: &Transaction<'_>, project: &str, prefix: &str) -> Result<String> {
    if !(12..=64).contains(&prefix.len())
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("hash_prefix");
    }
    let rows = tx.query("SELECT token_sha256 FROM oo_token_control.token_record WHERE project_id=$1 AND left(token_sha256,length($2))=$2 ORDER BY token_sha256 LIMIT 2", &[&project, &prefix]).await.map_err(|_| "database")?;
    unique_hash(rows.into_iter().map(|r| r.get(0)))
}
async fn operate(
    tx: &Transaction<'_>,
    mut op: Operation,
    actor: &str,
    now: i64,
    project: &str,
) -> Result<Receipt> {
    if !project.is_empty() {
        ident(project, 128)?;
    }
    // Called under the project lock; ambiguous prefixes can never mutate a token.
    match &mut op {
        Operation::Renew { hash, .. }
        | Operation::Rotate { hash, .. }
        | Operation::Revoke { hash, .. } => {
            *hash = resolve_hash(tx, project, hash).await?;
        }
        _ => (),
    }
    let p = policy(tx, project).await?;
    match op {
        Operation::Issue(g) => issue(tx, g, actor, now, &p, None, None).await,
        Operation::Policy { policy: p, .. } => {
            ident(project, 128)?;
            p.validate()?;
            tx.execute("INSERT INTO oo_token_lifecycle.policy VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(project_id) DO UPDATE SET max_lifetime_ms=$2,default_ttl_ms=$3,self_renew_enabled=$4,self_renew_window_ms=$5,rotate_overlap_ms=$6,enroll_ttl_ms=$7",&[&project,&p.max_lifetime_ms,&p.default_ttl_ms,&p.self_renew_enabled,&p.self_renew_window_ms,&p.rotate_overlap_ms,&p.enroll_ttl_ms]).await.map_err(|_|"database")?;
            Ok(Receipt::metadata(json!({"policy":p})))
        }
        Operation::List { .. } => {
            let rows=tx.query("SELECT t.token_sha256,t.role_name,t.absolute_deadline_ms,t.last_used_ms,COALESCE(x.first_issued_at_ms,t.issued_at_ms),x.overlap_until_ms FROM oo_token_control.token_record t LEFT JOIN oo_token_lifecycle.token x USING(token_sha256) WHERE project_id=$1 ORDER BY issued_at_ms,token_sha256",&[&project]).await.map_err(|_|"database")?;
            Ok(Receipt::metadata(
                json!({"tokens":rows.iter().map(|r|json!({"hash_prefix":&r.get::<_,String>(0)[..12],"role":r.get::<_,String>(1),"expires_at_ms":r.get::<_,i64>(2),"last_used_ms":r.get::<_,i64>(3),"first_issued_at_ms":r.get::<_,i64>(4),"overlap_until_ms":r.get::<_,Option<i64>>(5)})).collect::<Vec<_>>()}),
            ))
        }
        Operation::Renew {
            hash, deadline_ms, ..
        } => renew(tx, project, &hash, now, deadline_ms, &p).await,
        Operation::SelfRenew {
            token,
            audience: aud,
        } => {
            let (kind, hash) = legacy::token_hash(&token).map_err(|_| "input")?;
            if kind != TokenKind::Client {
                return Err("binding");
            }
            let r = live(tx, project, &hash, now).await?;
            if r.get::<_, String>("audience_url") != audience(&aud)? {
                return Err("audience");
            }
            if !p.self_renew_enabled {
                return Err("self_renew_disabled");
            }
            if r.get::<_, i64>("absolute_deadline_ms") - now > p.self_renew_window_ms {
                return Err("self_renew_window");
            }
            let ceiling = r
                .get::<_, i64>("first_ms")
                .checked_add(p.max_lifetime_ms)
                .ok_or("max_lifetime")?;
            let deadline = now
                .checked_add(p.default_ttl_ms)
                .ok_or("max_lifetime")?
                .min(ceiling);
            renew(tx, project, &hash, now, deadline, &p).await
        }
        Operation::Rotate { hash, .. } => {
            let r = live(tx, project, &hash, now).await?;
            if r.get::<_, Option<i64>>("overlap_until_ms").is_some() {
                return Err("already_rotated");
            }
            let first = r.get::<_, i64>("first_ms");
            let ttl = p
                .default_ttl_ms
                .min(first.checked_add(p.max_lifetime_ms).ok_or("max_lifetime")? - now);
            let g = Grant {
                project_id: project.into(),
                principal_id: r.get("principal_id"),
                role: r.get("role_name"),
                claims: serde_json::from_str(&r.get::<_, String>("claims_canonical"))
                    .map_err(|_| "claims")?,
                policy_version: r.get("policy_version"),
                knowledge_version: r.get("knowledge_version"),
                audience_url: r.get("audience_url"),
                ttl_ms: Some(ttl),
                purpose: "rotation".into(),
            };
            tx.execute("INSERT INTO oo_token_lifecycle.token (token_sha256,first_issued_at_ms,issued_by,purpose) VALUES ($1,$2,$3,'v1') ON CONFLICT DO NOTHING",&[&hash,&first,&actor]).await.map_err(|_|"database")?;
            let mut receipt = issue(tx, g, actor, now, &p, Some(&hash), Some(first)).await?;
            // Never lengthen the old credential. Zero overlap means immediate revocation.
            let overlap = now
                .checked_add(p.rotate_overlap_ms)
                .ok_or("input")?
                .min(r.get("absolute_deadline_ms"));
            tx.execute(
                "UPDATE oo_token_lifecycle.token SET overlap_until_ms=$1 WHERE token_sha256=$2",
                &[&overlap, &hash],
            )
            .await
            .map_err(|_| "database")?;
            if overlap <= r.get::<_, i64>("issued_at_ms") {
                revoke(tx, project, &hash, now).await?;
            } else {
                tx.execute("UPDATE oo_token_control.token_record SET absolute_deadline_ms=$1 WHERE token_sha256=$2",&[&overlap,&hash]).await.map_err(|_|"database")?;
            }
            receipt.metadata["overlap_until_ms"] = json!(overlap);
            Ok(receipt)
        }
        Operation::Revoke { hash, .. } => {
            revoke(tx, project, &hash, now).await?;
            Ok(Receipt::metadata(json!({"token_sha256":hash})))
        }
        Operation::Enroll(mut g) => {
            g.validate()?;
            let ttl = g.ttl_ms.unwrap_or(p.default_ttl_ms);
            if !(1..=p.max_lifetime_ms).contains(&ttl) {
                return Err("max_lifetime");
            }
            let mut raw = legacy::draw_32().map_err(|_| "randomness")?;
            const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
            let mut code = Zeroizing::new(String::from("OO-"));
            for (n, b) in raw.iter().take(8).enumerate() {
                if n == 4 {
                    code.push('-');
                }
                code.push(ALPHABET[(b & 31) as usize] as char);
            }
            raw.zeroize();
            let hash = digest(code.as_bytes());
            let expiry = now.checked_add(p.enroll_ttl_ms).ok_or("input")?;
            let grant = serde_json::to_string(&g).map_err(|_| "input")?;
            tx.execute("INSERT INTO oo_token_lifecycle.enrollment (code_sha256,project_id,grant_json,created_at_ms,expires_at_ms) VALUES ($1,$2,$3,$4,$5)",&[&hash,&project,&grant,&now,&expiry]).await.map_err(|_|"database")?;
            Ok(Receipt {
                metadata: json!({"expires_at_ms":expiry}),
                secret: Some(code),
            })
        }
        Operation::Redeem {
            code,
            audience: aud,
        } => {
            let hash = digest(code.as_bytes());
            let r=tx.query_opt("SELECT grant_json,created_at_ms,expires_at_ms,redeemed_at_ms FROM oo_token_lifecycle.enrollment WHERE code_sha256=$1 FOR UPDATE",&[&hash]).await.map_err(|_|"database")?.ok_or("not_found")?;
            if r.get::<_, Option<i64>>(3).is_some() {
                return Err("already_redeemed");
            }
            if now < r.get::<_, i64>(1) || now >= r.get::<_, i64>(2) {
                return Err("expired");
            }
            let g: Grant = serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| "input")?;
            if g.audience_url != audience(&aud)? {
                return Err("audience");
            }
            let receipt = issue(tx, g, actor, now, &p, None, None).await?;
            tx.execute("UPDATE oo_token_lifecycle.enrollment SET redeemed_at_ms=$1,redeemed_by=$2 WHERE code_sha256=$3",&[&now,&actor,&hash]).await.map_err(|_|"database")?;
            Ok(receipt)
        }
    }
}
async fn renew(
    tx: &Transaction<'_>,
    project: &str,
    hash: &str,
    now: i64,
    deadline: i64,
    p: &Policy,
) -> Result<Receipt> {
    let r = live(tx, project, hash, now).await?;
    let old: i64 = r.get("absolute_deadline_ms");
    if r.get::<_, Option<i64>>("overlap_until_ms").is_some() {
        return Err("already_rotated");
    }
    if deadline <= old
        || deadline
            > r.get::<_, i64>("first_ms")
                .checked_add(p.max_lifetime_ms)
                .ok_or("max_lifetime")?
    {
        return Err("max_lifetime");
    }
    tx.execute(
        "UPDATE oo_token_control.token_record SET absolute_deadline_ms=$1 WHERE token_sha256=$2",
        &[&deadline, &hash],
    )
    .await
    .map_err(|_| "database")?;
    Ok(Receipt::metadata(
        json!({"token_sha256":hash,"old_deadline_ms":old,"expires_at_ms":deadline}),
    ))
}
async fn revoke(tx: &Transaction<'_>, project: &str, hash: &str, now: i64) -> Result<()> {
    if tx
        .query_opt(
            "SELECT 1 FROM oo_token_control.token_record WHERE project_id=$1 AND token_sha256=$2",
            &[&project, &hash],
        )
        .await
        .map_err(|_| "database")?
        .is_none()
    {
        return Err("not_found");
    }
    let existing = tx.query_opt(
        "SELECT 1 FROM oo_token_control.revocation WHERE scope='token' AND project_id=$1 AND token_sha256=$2",
        &[&project, &hash],
    ).await.map_err(|_| "database")?;
    if existing.is_none() {
        tx.execute("INSERT INTO oo_token_control.revocation (scope,project_id,principal_id,session_id,token_sha256,seq,revoked_at_ms) VALUES ('token',$1,'','',$2,nextval('oo_token_control.revocation_seq'),$3)",&[&project,&hash,&now]).await.map_err(|_|"database")?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdminRequest {
    Issue {
        grant: Grant,
    },
    List {
        project: String,
    },
    Renew {
        project: String,
        hash: String,
        deadline_ms: i64,
    },
    Rotate {
        project: String,
        hash: String,
    },
    Revoke {
        project: String,
        hash: String,
    },
    Enroll {
        grant: Grant,
    },
    Policy {
        project: String,
        policy: Policy,
    },
}
impl From<AdminRequest> for Operation {
    fn from(r: AdminRequest) -> Self {
        match r {
            AdminRequest::Issue { grant } => Self::Issue(grant),
            AdminRequest::List { project } => Self::List { project },
            AdminRequest::Renew {
                project,
                hash,
                deadline_ms,
            } => Self::Renew {
                project,
                hash,
                deadline_ms,
            },
            AdminRequest::Rotate { project, hash } => Self::Rotate { project, hash },
            AdminRequest::Revoke { project, hash } => Self::Revoke { project, hash },
            AdminRequest::Enroll { grant } => Self::Enroll(grant),
            AdminRequest::Policy { project, policy } => Self::Policy { project, policy },
        }
    }
}

// Re-export the accepted read path so lifecycle clients can validate with the
// same five-second cache and frozen-tuple checks used by the Gateway.
pub use legacy::{
    CurrentTuple, DeclaredTransport, FixedWall, LookupCache, LookupRequest, lookup, lookup_for_test,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_prefix_resolution_refuses_missing_and_ambiguous_matches() {
        assert_eq!(
            unique_hash(Vec::<String>::new().into_iter()),
            Err("not_found")
        );
        assert_eq!(
            unique_hash(["a".repeat(64)].into_iter()),
            Ok("a".repeat(64))
        );
        assert_eq!(
            unique_hash(["a".repeat(64), "b".repeat(64)].into_iter()),
            Err("ambiguous_hash_prefix")
        );
    }
    #[test]
    fn issued_audiences_remain_compatible_with_the_accepted_reader() {
        assert_eq!(
            audience("https://GATEWAY.example:443"),
            Ok("https://gateway.example/".into())
        );
        for invalid in [
            "https://[::1]/",
            "https://example./",
            "https://a..b/",
            "https://gateway.example:99999/",
            "https://user@gateway.example/",
            "http://gateway.example/",
            "https://gateway.example/?secret=x",
        ] {
            assert!(audience(invalid).is_err());
        }
    }
}
