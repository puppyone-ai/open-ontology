//! Frozen F22 row-by-row receipt computation, test-only equivalence oracle.
//! Copied from the fixed F22 baseline, never reachable in a production build.
use super::*;

impl BindingCheck {
    pub(crate) async fn reference_verify(
        &self,
        tx: &Transaction<'_>,
    ) -> Result<BindingReceipt, ExecError> {
        let current = load_policy_catalog_in_transaction(
            tx,
            &self.identity,
            &self.policies,
            &self.limits,
            self.source.as_deref(),
            self.vector.as_ref(),
            self.native_catalog.as_ref(),
        )
        .await
        .map_err(|error| match error {
            CatalogError::Database { sqlstate } if sqlstate == "57014" => ExecError::Timeout {
                sqlstate: Some(sqlstate),
            },
            _ => drift(),
        })?;
        if self.admitted_catalog.as_ref() != Some(&current) {
            return Err(drift());
        }
        let mut data = Vec::new();
        let mut permissions = Vec::new();
        let mut sources = BTreeSet::new();
        for relation in &current.relations {
            let source = self
                .vector
                .as_ref()
                .and_then(Value::as_array)
                .and_then(|v| {
                    v.iter()
                        .find(|v| v["dataset"] == format!("{}.{}", relation.schema, relation.name))
                })
                .and_then(|v| v["source_id"].as_str())
                .or(self.source.as_deref())
                .unwrap_or(&self.identity.project);
            sources.insert(source.to_owned());
            let rows = tx
                .query(
                    "SELECT to_jsonb(f)::text FROM _oo.table_freshness f \
                 WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND physical_name=$4",
                    &[
                        &source,
                        &relation.schema,
                        &relation.name,
                        &relation.physical_name,
                    ],
                )
                .await
                .map_err(database)?;
            if rows.len() != 1 {
                return Err(drift());
            }
            let freshness: String = rows[0].get(0);
            let fence = tx
                .query(
                    "SELECT to_jsonb(f)::text FROM oo_publish_target.fence f \
                 WHERE role_name=$1 AND schema_name=$2 AND relation_name=$3 \
                 AND state='open' AND opened_pack_digest=$4",
                    &[
                        &self.identity.role,
                        &relation.physical_schema,
                        &relation.physical_name,
                        &self.pack_digest,
                    ],
                )
                .await
                .map_err(database)?;
            if fence.len() != 1 {
                return Err(drift());
            }
            data.push(json!([relation.schema, relation.name, freshness]));
            permissions.push(fence[0].get::<_, String>(0));
            let acl: String = tx.query_one(
                "SELECT jsonb_build_array(c.oid,c.relacl,c.relrowsecurity,c.relforcerowsecurity,c.relkind,c.reloptions, \
                 CASE WHEN c.relkind='v' THEN pg_catalog.pg_get_viewdef(c.oid,true) ELSE c.relfilenode::text END, \
                 (SELECT jsonb_agg(jsonb_build_array(a.attnum,a.attname,a.atttypid,a.atttypmod, \
                     a.attnotnull,a.attcollation,a.attgenerated,a.attidentity,a.attisdropped,a.attacl) ORDER BY a.attnum) \
                  FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0), \
                 (SELECT jsonb_agg(to_jsonb(p) ORDER BY p.oid) FROM pg_catalog.pg_policy p WHERE p.polrelid=c.oid))::text \
                 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
                 WHERE n.nspname=$1 AND c.relname=$2 \
                 AND c.oid=pg_catalog.to_regclass(pg_catalog.quote_ident($1)||'.'||pg_catalog.quote_ident($2)) \
                 AND (c.relkind='v' OR c.relfilenode=pg_catalog.pg_relation_filenode(c.oid))",
                &[&relation.physical_schema, &relation.physical_name],
            ).await.map_err(database)?.get(0);
            permissions.push(acl);
        }
        // Native C5 can publish data while retaining all semantic/Pack labels.
        // Bind the actual source positions as well as per-table generations.
        let has_watermarks: bool = tx
            .query_one(
                "SELECT pg_catalog.to_regclass('_oo.source_watermark') IS NOT NULL",
                &[],
            )
            .await
            .map_err(database)?
            .get(0);
        if has_watermarks {
            for source in sources {
                let rows = tx
                    .query(
                        "SELECT to_jsonb(w)::text FROM _oo.source_watermark w WHERE source=$1",
                        &[&source],
                    )
                    .await
                    .map_err(database)?;
                if rows.len() != 1 {
                    return Err(drift());
                }
                data.push(json!([
                    "source_watermark",
                    source,
                    rows[0].get::<_, String>(0)
                ]));
            }
        } else if self.vector.is_some() {
            return Err(drift());
        }
        data.sort_by_key(Value::to_string);
        permissions.sort();
        let schema = json!({
            "relations": current.relations.iter().map(|r| json!([
                r.schema, r.name, r.physical_schema, r.physical_name,
                r.columns.iter().map(|c| json!([c.name, c.physical_name, c.type_oid])).collect::<Vec<_>>()
            ])).collect::<Vec<_>>(),
            "functions": current.functions.iter().map(|f| json!([
                f.oid, f.schema, f.name, f.argument_type_oids, f.aggregate_star
            ])).collect::<Vec<_>>(),
            "operators": current.operators.iter().map(|o| json!([
                o.oid, o.procedure_oid, o.schema, o.name, o.left_type_oid, o.right_type_oid, o.result_type_oid
            ])).collect::<Vec<_>>(),
            "literal_types": [current.literal_types.integer, current.literal_types.text, current.literal_types.boolean],
            "null_test_allowed": current.null_test_allowed,
        });
        let receipt = receipt(
            &json!([
                self.identity.project,
                self.principal,
                self.identity.role,
                self.identity.policy_id,
                self.policy_version,
                self.knowledge_version,
                self.pack_digest,
                self.vector,
                self.native_catalog,
                permissions,
                schema
            ]),
            &json!(data),
        );
        if !self.expected.matches(&receipt) {
            return Err(drift());
        }
        Ok(receipt)
    }
}
