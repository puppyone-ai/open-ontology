//! Resolve scalar casts in the same read-only snapshot as the role catalog.
use super::*;
use crate::sql_guard::{ResolvedCast, SCALAR_CAST_TYPES};
use postgres_types::Type;

async fn function(
    tx: &Transaction<'_>,
    oid: i32,
    expected_result: u32,
) -> Result<ResolvedFunction, CatalogError> {
    let row = tx
        .query_opt(
            "SELECT p.proname, ARRAY(SELECT x::int4 FROM unnest(p.proargtypes) x) \
         FROM pg_catalog.pg_proc p \
         JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
         WHERE p.oid=($1::int4)::oid AND n.nspname='pg_catalog' \
           AND p.prokind='f' AND p.provolatile IN ('i','s') \
           AND NOT p.prosecdef AND NOT p.proretset AND p.provariadic=0 \
           AND p.proconfig IS NULL AND l.lanname='internal' \
           AND p.prorettype=($2::int4)::oid \
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d \
             WHERE d.classid='pg_catalog.pg_proc'::regclass AND d.objid=p.oid \
               AND d.deptype='e')",
            &[&oid, &(expected_result as i32)],
        )
        .await
        .map_err(db_error)?
        .ok_or(CatalogError::MissingObject)?;
    Ok(ResolvedFunction {
        oid: oid_from_i32(oid)?,
        schema: "pg_catalog".into(),
        name: row.get(0),
        argument_type_oids: row
            .get::<_, Vec<i32>>(1)
            .into_iter()
            .map(oid_from_i32)
            .collect::<Result<_, _>>()?,
        aggregate_star: false,
    })
}

pub(super) async fn load(tx: &Transaction<'_>) -> Result<Vec<ResolvedCast>, CatalogError> {
    let mut io = BTreeMap::new();
    for ty in SCALAR_CAST_TYPES {
        let row = tx
            .query_opt(
                "SELECT t.typinput::int4,t.typoutput::int4 FROM pg_catalog.pg_type t \
             JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace \
             WHERE t.oid=($1::int4)::oid AND t.typname=$2 AND n.nspname='pg_catalog' \
               AND t.typtype='b' AND t.typbasetype=0 \
               AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d \
                 WHERE d.classid='pg_catalog.pg_type'::regclass AND d.objid=t.oid \
                   AND d.deptype='e')",
                &[&(ty.oid() as i32), &ty.name()],
            )
            .await
            .map_err(db_error)?
            .ok_or(CatalogError::MissingObject)?;
        let input = function(tx, row.get(0), ty.oid()).await?;
        let output = function(tx, row.get(1), Type::CSTRING.oid()).await?;
        let prefix = match ty.name() {
            "numeric" | "date" | "timestamp" | "timestamptz" | "interval" => {
                format!("{}_", ty.name())
            }
            name => name.to_owned(),
        };
        if input.name != format!("{prefix}in")
            || output.name != format!("{prefix}out")
            || input.argument_type_oids.first() != Some(&Type::CSTRING.oid())
            || output.argument_type_oids != [ty.oid()]
        {
            return Err(CatalogError::MissingObject);
        }
        io.insert(ty.oid(), (input, output));
    }
    let oids: Vec<i32> = SCALAR_CAST_TYPES.iter().map(|ty| ty.oid() as i32).collect();
    let rows = tx
        .query(
            "SELECT c.castsource::int4,c.casttarget::int4,c.castmethod::text,c.castfunc::int4,\
           EXISTS (SELECT 1 FROM pg_catalog.pg_depend d \
             WHERE d.classid='pg_catalog.pg_cast'::regclass AND d.objid=c.oid \
               AND d.deptype='e') \
         FROM pg_catalog.pg_cast c \
         WHERE c.castsource=ANY($1::int4[]::oid[]) AND c.casttarget=ANY($1::int4[]::oid[])",
            &[&oids],
        )
        .await
        .map_err(db_error)?;
    let mut declared = BTreeSet::new();
    let mut casts = Vec::new();
    for row in rows {
        let source = oid_from_i32(row.get(0))?;
        let target = oid_from_i32(row.get(1))?;
        if !declared.insert((source, target)) || row.get::<_, bool>(4) {
            return Err(CatalogError::AmbiguousObject);
        }
        if source == target {
            // Typmod coercions are not granted by this no-typmod grammar.
            continue;
        }
        let functions = match row.get::<_, String>(2).as_str() {
            "f" => {
                // PostgreSQL reuses text(bool) for the binary-compatible
                // bool-to-varchar cast; the cast still returns varchar.
                let result = if source == Type::BOOL.oid() && target == Type::VARCHAR.oid() {
                    Type::TEXT.oid()
                } else {
                    target
                };
                let f = function(tx, row.get(3), result).await?;
                let target_name = Type::from_oid(result).ok_or(CatalogError::MissingObject)?;
                if f.name != target_name.name() || f.argument_type_oids != [source] {
                    return Err(CatalogError::MissingObject);
                }
                vec![f]
            }
            "b" if [source, target]
                .iter()
                .all(|oid| [Type::TEXT.oid(), Type::VARCHAR.oid()].contains(oid)) =>
            {
                Vec::new()
            }
            "i" => vec![io[&source].1.clone(), io[&target].0.clone()],
            _ => return Err(CatalogError::MissingObject),
        };
        casts.push(ResolvedCast {
            source_type_oid: source,
            target_type_oid: target,
            functions,
        });
    }
    for target in SCALAR_CAST_TYPES {
        casts.push(ResolvedCast {
            source_type_oid: 0,
            target_type_oid: target.oid(),
            functions: vec![io[&target.oid()].0.clone()],
        });
        casts.push(ResolvedCast {
            source_type_oid: target.oid(),
            target_type_oid: target.oid(),
            functions: Vec::new(),
        });
        for source in SCALAR_CAST_TYPES {
            if source == target || declared.contains(&(source.oid(), target.oid())) {
                continue;
            }
            // PostgreSQL's explicit CoerceViaIO fallback for string types.
            if [source, target]
                .iter()
                .any(|ty| **ty == Type::TEXT || **ty == Type::VARCHAR)
            {
                casts.push(ResolvedCast {
                    source_type_oid: source.oid(),
                    target_type_oid: target.oid(),
                    functions: vec![io[&source.oid()].1.clone(), io[&target.oid()].0.clone()],
                });
            }
        }
    }
    Ok(casts)
}
