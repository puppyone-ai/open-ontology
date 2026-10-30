//! Bounded, transaction-local bulk reads. No values survive the caller's
//! snapshot; grants, mappings, types and publication proofs are still live.
use super::*;

pub(super) async fn ensure_generations(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    native_vector: bool,
) -> Result<(), CatalogError> {
    let sources: Vec<_> = bound.tables.iter().map(|t| t.source.clone()).collect();
    let digests: Vec<_> = bound
        .tables
        .iter()
        .map(|t| t.config_digest.clone())
        .collect();
    let generations: Vec<_> = bound.tables.iter().map(|t| t.generation).collect();
    let namespaces: Vec<_> = bound.tables.iter().map(|t| t.namespace.clone()).collect();
    let tables: Vec<_> = bound.tables.iter().map(|t| t.name.clone()).collect();
    let valid: bool = tx.query_one(
        "SELECT COALESCE(bool_and(w.generation IS NOT DISTINCT FROM \
            (SELECT max(m.run_generation) FROM _oo.physical_mappings m \
             WHERE m.source=w.source AND m.config_digest=w.digest \
               AND (NOT $6 OR (m.level='t' AND m.logical_namespace=w.namespace AND m.logical_table=w.table_name)))),false) \
         FROM unnest($1::text[],$2::text[],$3::int8[],$4::text[],$5::text[]) \
         AS w(source,digest,generation,namespace,table_name)",
        &[&sources,&digests,&generations,&namespaces,&tables,&native_vector],
    ).await.map_err(db_error)?.get(0);
    if valid {
        Ok(())
    } else {
        Err(CatalogError::StaleGeneration)
    }
}

pub(super) struct SnapshotAttribute {
    pub attribute: Attribute,
    pub allowed: bool,
    pub formatted: String,
}

pub(super) struct SnapshotRelation {
    pub relation: RelationRow,
    pub attributes: Vec<SnapshotAttribute>,
}

pub(super) struct Relations {
    by_mapping: BTreeMap<(String, String), i32>,
    by_oid: BTreeMap<i32, SnapshotRelation>,
}

impl Relations {
    pub fn get(&self, schema: &str, physical: &str) -> Result<&SnapshotRelation, CatalogError> {
        self.by_mapping
            .get(&(schema.into(), physical.into()))
            .and_then(|oid| self.by_oid.get(oid))
            .ok_or(CatalogError::MappingAbsent)
    }

    pub async fn load(
        tx: &Transaction<'_>,
        bound: &BoundInput,
        mappings: &MappingIndex,
        limits: &CatalogLimits,
    ) -> Result<Self, CatalogError> {
        let mut wanted = BTreeSet::new();
        for table in &bound.tables {
            let scoped = table_scope(bound, table);
            let schema = if bound.native_namespace {
                mappings.resolve(
                    &scoped.source,
                    &scoped.config_digest,
                    scoped.generation,
                    LEVEL_NAMESPACE,
                    true,
                    &table.namespace,
                    "",
                    "",
                )?
            } else {
                PHYSICAL_SCHEMA.into()
            };
            let physical = mappings.resolve(
                &scoped.source,
                &scoped.config_digest,
                scoped.generation,
                LEVEL_TABLE,
                scoped.native_namespace,
                &table.namespace,
                &table.name,
                "",
            )?;
            wanted.insert((schema, physical));
        }
        let (schemas, names): (Vec<_>, Vec<_>) = wanted.iter().cloned().unzip();
        let rows = tx.query(
            "SELECT w.ordinality::int8,c.oid::int4,c.reltuples,n.nspname,c.relkind::text \
             FROM unnest($1::text[],$2::text[]) WITH ORDINALITY w(schema_name,relation_name,ordinality) \
             JOIN pg_catalog.pg_namespace n ON (n.nspname=w.schema_name OR ($3 AND n.nspname='_oo')) \
             JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=w.relation_name \
                AND (c.relkind='r' OR ($3 AND c.relkind='v')) ORDER BY w.ordinality,c.oid",
            &[&schemas,&names,&bound.native_namespace],
        ).await.map_err(db_error)?;
        let mut found = BTreeMap::<usize, (RelationRow, String)>::new();
        for row in rows {
            let idx = usize::try_from(row.get::<_, i64>(0))
                .ok()
                .and_then(|i| i.checked_sub(1))
                .filter(|i| *i < schemas.len())
                .ok_or(CatalogError::Snapshot)?;
            let relation = RelationRow {
                oid: row.get(1),
                reltuples: row.get(2),
                schema: row.get(3),
            };
            if found.insert(idx, (relation, row.get(4))).is_some() {
                return Err(CatalogError::AmbiguousMapping);
            }
        }
        if found.len() != schemas.len() {
            return Err(CatalogError::MappingAbsent);
        }
        let views: Vec<String> = found
            .iter()
            .filter(|(_, (r, kind))| kind == "v" && bound.native_namespace && r.schema == "_oo")
            .map(|(idx, _)| names[*idx].clone())
            .collect();
        let mut overlays = BTreeMap::new();
        if !views.is_empty() {
            let installed: bool = tx
                .query_one(
                    "SELECT to_regprocedure('_oo.c5_overlay_is_valid(text)') IS NOT NULL \
                 AND to_regprocedure('_oo.c5_overlay_row_count(text)') IS NOT NULL",
                    &[],
                )
                .await
                .map_err(db_error)?
                .get(0);
            if installed {
                for row in tx.query(
                    "SELECT v,_oo.c5_overlay_is_valid(v),_oo.c5_overlay_row_count(v) FROM unnest($1::text[]) AS v",
                    &[&views],
                ).await.map_err(db_error)? {
                    overlays.insert(row.get::<_,String>(0),(row.get::<_,bool>(1),row.get::<_,Option<i64>>(2)));
                }
            }
        }
        let mut by_mapping = BTreeMap::new();
        let mut by_oid = BTreeMap::new();
        for (idx, (mut relation, kind)) in found {
            let registered = if kind == "v" && bound.native_namespace && relation.schema == "_oo" {
                match overlays.get(&names[idx]) {
                    Some((true, Some(count))) if *count >= 0 => {
                        relation.reltuples = *count as f32;
                        true
                    }
                    Some((true, _)) => return Err(CatalogError::MappingAbsent),
                    _ => false,
                }
            } else {
                false
            };
            if !admitted_native_relation(&kind, bound.native_namespace, registered) {
                return Err(CatalogError::MappingAbsent);
            }
            by_mapping.insert((schemas[idx].clone(), names[idx].clone()), relation.oid);
            by_oid.entry(relation.oid).or_insert(SnapshotRelation {
                relation,
                attributes: Vec::new(),
            });
        }
        let oids: Vec<i32> = by_oid.keys().copied().collect();
        let rows=tx.query(
            "SELECT a.attrelid::int4,a.attnum,a.attname::text,a.atttypid::int4,a.atttypmod::int4,t.typname::text,t.typtype::text, \
                pg_catalog.has_column_privilege(($2::text)::name,a.attrelid,a.attnum,'SELECT'), \
                pg_catalog.format_type(a.atttypid,a.atttypmod) \
             FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_type t ON t.oid=a.atttypid \
             WHERE a.attrelid=ANY($1::oid[]) AND a.attnum>0 AND NOT a.attisdropped \
             ORDER BY a.attrelid,a.attnum",
            &[&oids.iter().map(|oid|*oid as u32).collect::<Vec<_>>(),&bound.role],
        ).await.map_err(db_error)?;
        for row in rows {
            let relation = by_oid
                .get_mut(&row.get::<_, i32>(0))
                .ok_or(CatalogError::Snapshot)?;
            relation.attributes.push(SnapshotAttribute {
                attribute: Attribute {
                    attnum: row.get(1),
                    name: row.get(2),
                    type_oid: oid_from_i32(row.get(3))?,
                    typmod: row.get(4),
                    type_name: row.get(5),
                    type_kind: row.get(6),
                },
                allowed: row.get::<_, Option<bool>>(7) == Some(true),
                formatted: row.get(8),
            });
            if relation.attributes.len() > limits.max_columns as usize {
                return Err(CatalogError::LimitExceeded);
            }
        }
        Ok(Self { by_mapping, by_oid })
    }
}

pub(super) struct LibraryProofs {
    by_dataset: BTreeMap<String, (bool, i64)>,
}

impl LibraryProofs {
    pub fn verify(&self, dataset: &str) -> Result<(), CatalogError> {
        match self.by_dataset.get(dataset) {
            Some((false, _)) => Err(CatalogError::PublicationBindingUnavailable),
            Some((true, 1)) => Ok(()),
            _ => Err(CatalogError::IdentityMismatch),
        }
    }

    pub async fn load(
        tx: &Transaction<'_>,
        bound: &BoundInput,
        entries: &BTreeMap<&str, &Value>,
        catalog: &Value,
        mappings: &MappingIndex,
        relations: &Relations,
    ) -> Result<Self, CatalogError> {
        let pack = catalog["pack_digest"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        let cut = crate::publication_binding::library_cut(tx, catalog).await
            .map_err(|_| CatalogError::PublicationBindingUnavailable)?;
        let mut run_ids = Vec::new();
        let mut sources = Vec::new();
        let mut generations = Vec::new();
        let mut targets = Vec::new();
        let mut digests = Vec::new();
        let mut namespaces = Vec::new();
        let mut tables = Vec::new();
        let mut physicals = Vec::new();
        let mut actual_schemas = Vec::new();
        let mut types = Vec::new();
        let mut inputs = Vec::new();
        let mut datasets = Vec::new();
        let mut receipts = Vec::new();
        for table in &bound.tables {
            let dataset = format!("{}.{}", table.namespace, table.name);
            let entry = entries.get(dataset.as_str()).ok_or(CatalogError::IdentityMismatch)?;
            let text = |key: &str| entry[key].as_str().ok_or(CatalogError::IdentityMismatch);
            if text("source_id")? != table.source
                || text("config_digest")? != table.config_digest
                || entry["source_generation"].as_i64() != Some(table.generation)
            {
                return Err(CatalogError::IdentityMismatch);
            }
            let (receipt, published) = crate::publication_binding::library_table(
                &cut, &table.source, &table.namespace, &table.name,
            ).map_err(|_| CatalogError::IdentityMismatch)?;
            if ["run_id","source_generation","target_generation"].iter()
                .any(|key| published[*key] != entry[*key])
                || published["physical_name"] != entry["stored_physical"] {
                return Err(CatalogError::IdentityMismatch);
            }
            // C1 storage is current-checked separately; its receipt need not
            // have the original import's count/position.
            receipts.push(if receipt["consistency"] == "per_table" {
                published.to_string()
            } else { "null".into() });
            let namespace = mappings.resolve(
                &table.source,
                &table.config_digest,
                table.generation,
                LEVEL_NAMESPACE,
                bound.native_namespace,
                &table.namespace,
                "",
                "",
            )?;
            let physical = mappings.resolve(
                &table.source,
                &table.config_digest,
                table.generation,
                LEVEL_TABLE,
                bound.native_namespace,
                &table.namespace,
                &table.name,
                "",
            )?;
            let relation = &relations.get(&namespace, &physical)?.relation;
            if relation.schema != text("stored_namespace")? || physical != text("stored_physical")?
            {
                return Err(CatalogError::IdentityMismatch);
            }
            datasets.push(dataset);
            run_ids.push(text("run_id")?.to_owned());
            sources.push(table.source.clone());
            generations.push(table.generation);
            targets.push(
                entry["target_generation"]
                    .as_i64()
                    .ok_or(CatalogError::IdentityMismatch)?,
            );
            digests.push(table.config_digest.clone());
            namespaces.push(table.namespace.clone());
            tables.push(table.name.clone());
            physicals.push(physical);
            actual_schemas.push(relation.schema.clone());
            types.push(text("type_digest")?.to_owned());
            inputs.push(text("input_digest")?.to_owned());
        }
        // Keep exact text equality as well: the UUID index narrows the lookup,
        // but PostgreSQL's permissive UUID spelling must not widen the binding.
        let rows=tx.query(
            "SELECT w.ordinality::int8, \
                EXISTS(SELECT 1 FROM oo_publish_target.fence f WHERE f.schema_name=w.actual_schema \
                    AND f.relation_name=w.physical AND f.role_name=$12 AND f.state='open' AND f.opened_pack_digest=$13), \
                (SELECT count(*)::int8 FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) \
                 WHERE r.run_id=w.run_id::uuid AND r.run_id::text=w.run_id \
                    AND r.source_id=w.source AND r.source_generation=w.generation \
                    AND r.target_generation=w.target AND r.config_digest=w.digest AND r.sealed_at IS NOT NULL \
                    AND r.state='published' AND r.type_digest=w.type_digest AND r.input_digest=w.input_digest \
                    AND t.schema_ident=w.namespace AND t.table_ident=w.table_name AND t.physical_name=w.physical \
                    AND t.exhausted AND (w.receipt='null' OR (t.canonical_digest=w.receipt::jsonb->>'canonical_digest' \
                    AND t.applied_rows=(w.receipt::jsonb->>'accepted_rows')::bigint \
                    AND t.applied_pos::text=w.receipt::jsonb->>'applied_pos')) \
                    AND EXISTS(SELECT 1 FROM _oo.apply_receipts a WHERE a.source=r.source_id \
                        AND a.table_generation=t.physical_name AND a.config_digest=r.config_digest \
                        AND a.schema_digest=r.schema_digest AND a.type_digest=r.type_digest \
                        AND a.fence_generation=r.target_generation AND a.fence_shard=r.shard_id AND NOT a.partial_txn)) \
             FROM unnest($1::text[],$2::text[],$3::int8[],$4::int8[],$5::text[],$6::text[],$7::text[], \
                $8::text[],$9::text[],$10::text[],$11::text[],$14::text[]) WITH ORDINALITY \
                w(run_id,source,generation,target,digest,namespace,table_name,physical,actual_schema,type_digest,input_digest,receipt,ordinality)",
            &[&run_ids,&sources,&generations,&targets,&digests,&namespaces,&tables,&physicals,&actual_schemas,&types,&inputs,&bound.role,&pack,&receipts],
        ).await.map_err(db_error)?;
        let mut by_dataset = BTreeMap::new();
        for row in rows {
            let idx = usize::try_from(row.get::<_, i64>(0))
                .ok()
                .and_then(|i| i.checked_sub(1))
                .filter(|i| *i < datasets.len())
                .ok_or(CatalogError::Snapshot)?;
            if by_dataset
                .insert(datasets[idx].clone(), (row.get(1), row.get(2)))
                .is_some()
            {
                return Err(CatalogError::Snapshot);
            }
        }
        if by_dataset.len() != datasets.len() {
            return Err(CatalogError::Snapshot);
        }
        Ok(Self { by_dataset })
    }
}
