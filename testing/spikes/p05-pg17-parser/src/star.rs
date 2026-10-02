use std::collections::BTreeMap;

use pg_query::protobuf::node::Node as N;
use pg_query::protobuf::{
    ColumnRef, CommonTableExpr, JoinExpr, JoinType, Node, RangeFunction, RangeSubselect,
    RangeTableFunc, RangeVar, ResTarget, SelectStmt, SetOperation,
};

use crate::policy::{forbidden_name, Policy};

#[derive(Clone, Debug)]
struct OutCol {
    qualifier: String,
    name: String,
    large_base: bool,
}

#[derive(Clone, Debug)]
struct Source {
    name: String,
    cols: Vec<OutCol>,
}

#[derive(Clone, Debug)]
struct FromItem {
    stars: Vec<OutCol>,
    sources: Vec<Source>,
}

/// Qualified names use `sources`. Unqualified names use `outputs` (star shape).
#[derive(Clone, Debug, Default)]
struct ScopeView {
    sources: Vec<Source>,
    outputs: Vec<OutCol>,
}

pub struct StarOutcome {
    pub original_projection_star: bool,
    pub column_refs: u32,
}

struct Outcome {
    original_projection_star: bool,
}

pub fn rewrite_query(select: &mut SelectStmt, policy: &Policy) -> Result<StarOutcome, &'static str> {
    let mut visits = 0u32;
    let mut ctes = BTreeMap::new();
    let outcome = rewrite_select(select, policy, &mut ctes, &[], false, &mut visits)?;
    Ok(StarOutcome {
        original_projection_star: outcome.original_projection_star,
        column_refs: visits,
    })
}

fn rewrite_select(
    sel: &mut SelectStmt,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    outer: &[ScopeView],
    need_names: bool,
    visits: &mut u32,
) -> Result<Outcome, &'static str> {
    if sel.into_clause.is_some() {
        return Err("SELECT_INTO");
    }
    if !sel.locking_clause.is_empty() {
        return Err("ROW_LOCK");
    }
    if sel.with_clause.as_ref().is_some_and(|with| with.recursive) {
        return Err("RECURSIVE_BINDING_DEFERRED");
    }
    let mut local_ctes = ctes.clone();
    let mut nested_star = false;
    if let Some(with) = sel.with_clause.as_mut() {
        for cte_node in &mut with.ctes {
            let Some(N::CommonTableExpr(cte)) = cte_node.node.as_mut() else {
                return Err("UNKNOWN_NODE");
            };
            nested_star |= bind_cte(cte, policy, &mut local_ctes, outer, visits)?;
        }
    }

    let op = SetOperation::try_from(sel.op).unwrap_or(SetOperation::Undefined);
    if matches!(op, SetOperation::SetopUnion | SetOperation::SetopIntersect | SetOperation::SetopExcept) {
        let Some(left) = sel.larg.as_mut() else {
            return Err("UNKNOWN_NODE");
        };
        nested_star |= rewrite_select(left, policy, &mut local_ctes, outer, true, visits)?.original_projection_star;
        let Some(right) = sel.rarg.as_mut() else {
            return Err("UNKNOWN_NODE");
        };
        nested_star |= rewrite_select(right, policy, &mut local_ctes, outer, true, visits)?.original_projection_star;
        let names = {
            let left = sel.larg.as_ref().ok_or("UNKNOWN_NODE")?;
            output_names(&left.target_list)?
        };
        let order_view = ScopeView {
            sources: Vec::new(),
            outputs: names
                .into_iter()
                .map(|name| OutCol { qualifier: String::new(), name, large_base: false })
                .collect(),
        };
        let mut scopes = vec![order_view];
        scopes.extend(outer.iter().cloned());
        walk_opt(sel.where_clause.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
        walk_list(&mut sel.sort_clause, &scopes, policy, &mut local_ctes, visits)?;
        walk_list(&mut sel.group_clause, &scopes, policy, &mut local_ctes, visits)?;
        walk_list(&mut sel.distinct_clause, &scopes, policy, &mut local_ctes, visits)?;
        walk_list(&mut sel.window_clause, &scopes, policy, &mut local_ctes, visits)?;
        walk_opt(sel.limit_count.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
        walk_opt(sel.limit_offset.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
        walk_list(&mut sel.values_lists, &scopes, policy, &mut local_ctes, visits)?;
        return Ok(Outcome { original_projection_star: nested_star });
    }

    let mut items = Vec::new();
    for item in &mut sel.from_clause {
        let (from_item, star) = rewrite_from(item, policy, &mut local_ctes, outer, visits)?;
        nested_star |= star;
        items.push(from_item);
    }
    let local = view_of(&items);
    let mut scopes = vec![local];
    scopes.extend(outer.iter().cloned());
    let (targets, saw_star, touches_large) = rewrite_targets(&sel.target_list, &items, &scopes, policy, &mut local_ctes, visits)?;
    sel.target_list = targets;
    walk_opt(sel.where_clause.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
    walk_opt(sel.having_clause.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
    walk_list(&mut sel.group_clause, &scopes, policy, &mut local_ctes, visits)?;
    walk_list(&mut sel.sort_clause, &scopes, policy, &mut local_ctes, visits)?;
    walk_list(&mut sel.distinct_clause, &scopes, policy, &mut local_ctes, visits)?;
    walk_list(&mut sel.window_clause, &scopes, policy, &mut local_ctes, visits)?;
    walk_opt(sel.limit_count.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
    walk_opt(sel.limit_offset.as_deref_mut(), &scopes, policy, &mut local_ctes, visits)?;
    walk_list(&mut sel.values_lists, &scopes, policy, &mut local_ctes, visits)?;
    if saw_star && touches_large && sel.where_clause.is_none() {
        return Err("R9_STAR_ON_LARGE");
    }
    if need_names {
        let _ = output_names(&sel.target_list)?;
    }
    Ok(Outcome { original_projection_star: saw_star || nested_star })
}

fn view_of(items: &[FromItem]) -> ScopeView {
    ScopeView {
        sources: items.iter().flat_map(|item| item.sources.clone()).collect(),
        outputs: items.iter().flat_map(|item| item.stars.clone()).collect(),
    }
}

fn bind_cte(
    cte: &mut CommonTableExpr,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    outer: &[ScopeView],
    visits: &mut u32,
) -> Result<bool, &'static str> {
    if cte.cterecursive {
        return Err("RECURSIVE_BINDING_DEFERRED");
    }
    let name = cte.ctename.clone();
    if name.is_empty() {
        return Err("SCOPE_UNRESOLVED");
    }
    let Some(query) = cte.ctequery.as_mut() else {
        return Err("UNKNOWN_NODE");
    };
    let Some(N::SelectStmt(select)) = query.node.as_mut() else {
        return Err("SCOPE_UNRESOLVED");
    };
    let outcome = rewrite_select(select, policy, ctes, outer, true, visits)?;
    let cols: Vec<OutCol> = output_names(&select.target_list)?
        .into_iter()
        .map(|col| OutCol { qualifier: name.clone(), name: col, large_base: false })
        .collect();
    ctes.insert(name, cols);
    Ok(outcome.original_projection_star)
}

fn rewrite_from(
    node: &mut Node,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    outer: &[ScopeView],
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    match node.node.as_mut() {
        Some(N::RangeVar(range)) => Ok((source_from_range(range, policy, ctes)?, false)),
        Some(N::JoinExpr(join)) => source_from_join(join, policy, ctes, outer, visits),
        Some(N::RangeSubselect(sub)) => source_from_subselect(sub, policy, ctes, visits),
        Some(N::RangeTableFunc(func)) => source_from_table_func(func, policy, ctes, visits),
        Some(N::RangeFunction(func)) => source_from_function(func, policy, ctes, visits),
        Some(N::JsonTable(table)) => source_from_json_table(table, policy, ctes, visits),
        _ => Err("SCOPE_UNRESOLVED"),
    }
}

fn source_from_range(
    range: &RangeVar,
    policy: &Policy,
    ctes: &BTreeMap<String, Vec<OutCol>>,
) -> Result<FromItem, &'static str> {
    if !range.catalogname.is_empty() {
        return Err("FORBIDDEN_SCHEMA");
    }
    let alias = alias_name(range.alias.as_ref());
    let colnames = alias_colnames(range.alias.as_ref());
    if range.schemaname.is_empty() {
        let Some(found) = ctes.get(&range.relname) else {
            return Err("UNQUALIFIED_RELATION");
        };
        let name = alias.unwrap_or_else(|| range.relname.clone());
        let mut cols = found.clone();
        for col in &mut cols {
            col.qualifier = name.clone();
            col.large_base = false;
        }
        apply_colnames(&mut cols, &colnames)?;
        return Ok(named_item(name, cols));
    }
    if forbidden_name(&range.schemaname, &range.relname) {
        return Err("FORBIDDEN_SCHEMA");
    }
    let key = format!("{}.{}", range.schemaname, range.relname);
    let Some(rel) = policy.relations.get(&key) else {
        return Err("RELATION_NOT_FOUND");
    };
    let name = alias.unwrap_or_else(|| range.relname.clone());
    let mut cols: Vec<OutCol> = rel
        .visible
        .iter()
        .map(|col| OutCol { qualifier: name.clone(), name: col.clone(), large_base: rel.large })
        .collect();
    apply_colnames(&mut cols, &colnames)?;
    Ok(named_item(name, cols))
}

fn named_item(name: String, cols: Vec<OutCol>) -> FromItem {
    FromItem {
        stars: cols.clone(),
        sources: vec![Source { name, cols }],
    }
}

fn source_from_join(
    join: &mut JoinExpr,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    outer: &[ScopeView],
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    if join.is_natural {
        return Err("NATURAL_JOIN_DEFERRED");
    }
    if join.join_using_alias.is_some() {
        return Err("SCOPE_UNRESOLVED");
    }
    let Some(left_node) = join.larg.as_mut() else {
        return Err("SCOPE_UNRESOLVED");
    };
    let (left, mut star) = rewrite_from(left_node, policy, ctes, outer, visits)?;
    let Some(right_node) = join.rarg.as_mut() else {
        return Err("SCOPE_UNRESOLVED");
    };
    let (right, right_star) = rewrite_from(right_node, policy, ctes, outer, visits)?;
    star |= right_star;
    let on_view = ScopeView {
        sources: [left.sources.clone(), right.sources.clone()].concat(),
        outputs: [left.stars.clone(), right.stars.clone()].concat(),
    };
    let mut on_scopes = vec![on_view];
    on_scopes.extend(outer.iter().cloned());
    if let Some(quals) = join.quals.as_mut() {
        walk_node(quals, &on_scopes, policy, ctes, visits)?;
    }
    let using = using_names(&join.using_clause)?;
    let mut stars = if using.is_empty() {
        let mut cols = left.stars.clone();
        cols.extend(right.stars.clone());
        cols
    } else {
        let kind = JoinType::try_from(join.jointype).unwrap_or(JoinType::Undefined);
        if kind != JoinType::JoinInner {
            return Err("JOIN_USING_OUTER_DEFERRED");
        }
        let mut cols = Vec::new();
        for name in &using {
            let Some(left_col) = left.stars.iter().find(|col| col.name == *name) else {
                return Err("COLUMN_NOT_ALLOWED");
            };
            if !right.stars.iter().any(|col| col.name == *name) {
                return Err("COLUMN_NOT_ALLOWED");
            }
            cols.push(left_col.clone());
        }
        cols.extend(left.stars.iter().filter(|col| !using.iter().any(|name| name == &col.name)).cloned());
        cols.extend(right.stars.iter().filter(|col| !using.iter().any(|name| name == &col.name)).cloned());
        cols
    };
    let item = if let Some(name) = alias_name(join.alias.as_ref()) {
        apply_colnames(&mut stars, &alias_colnames(join.alias.as_ref()))?;
        for col in &mut stars {
            col.qualifier = name.clone();
        }
        named_item(name, stars)
    } else {
        let mut sources = left.sources.clone();
        sources.extend(right.sources);
        FromItem { stars, sources }
    };
    Ok((item, star))
}

fn source_from_subselect(
    sub: &mut RangeSubselect,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    if sub.lateral {
        return Err("LATERAL_DEFERRED");
    }
    let name = alias_name(sub.alias.as_ref()).ok_or("SCOPE_UNRESOLVED")?;
    let colnames = alias_colnames(sub.alias.as_ref());
    let Some(query) = sub.subquery.as_mut() else {
        return Err("UNKNOWN_NODE");
    };
    let Some(N::SelectStmt(select)) = query.node.as_mut() else {
        return Err("SCOPE_UNRESOLVED");
    };
    // Derived tables do not see outer columns. large_base stops here.
    let outcome = rewrite_select(select, policy, ctes, &[], true, visits)?;
    let mut cols: Vec<OutCol> = output_names(&select.target_list)?
        .into_iter()
        .map(|col| OutCol { qualifier: name.clone(), name: col, large_base: false })
        .collect();
    apply_colnames(&mut cols, &colnames)?;
    Ok((named_item(name, cols), outcome.original_projection_star))
}

fn source_from_table_func(
    func: &mut RangeTableFunc,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    if func.lateral {
        return Err("LATERAL_DEFERRED");
    }
    walk_closed(func.docexpr.as_deref_mut(), policy, ctes, visits)?;
    walk_closed(func.rowexpr.as_deref_mut(), policy, ctes, visits)?;
    for ns in &mut func.namespaces {
        walk_closed(Some(ns), policy, ctes, visits)?;
    }
    let name = alias_name(func.alias.as_ref()).ok_or("SCOPE_UNRESOLVED")?;
    let mut cols = Vec::new();
    for column in &mut func.columns {
        let Some(N::RangeTableFuncCol(col)) = column.node.as_mut() else {
            return Err("SCOPE_UNRESOLVED");
        };
        walk_closed(col.colexpr.as_deref_mut(), policy, ctes, visits)?;
        walk_closed(col.coldefexpr.as_deref_mut(), policy, ctes, visits)?;
        if col.colname.is_empty() {
            return Err("SCOPE_UNRESOLVED");
        }
        cols.push(OutCol { qualifier: name.clone(), name: col.colname.clone(), large_base: false });
    }
    if cols.is_empty() {
        return Err("SCOPE_UNRESOLVED");
    }
    Ok((named_item(name, cols), false))
}

fn source_from_json_table(
    table: &mut pg_query::protobuf::JsonTable,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    if table.lateral {
        return Err("LATERAL_DEFERRED");
    }
    if let Some(context) = table.context_item.as_mut() {
        walk_closed(context.raw_expr.as_deref_mut(), policy, ctes, visits)?;
        walk_closed(context.formatted_expr.as_deref_mut(), policy, ctes, visits)?;
    }
    if let Some(path) = table.pathspec.as_mut() {
        walk_closed(path.string.as_deref_mut(), policy, ctes, visits)?;
    }
    for passing in &mut table.passing {
        walk_closed(Some(passing), policy, ctes, visits)?;
    }
    let name = alias_name(table.alias.as_ref()).ok_or("SCOPE_UNRESOLVED")?;
    let mut cols = Vec::new();
    for column in &mut table.columns {
        let Some(N::JsonTableColumn(col)) = column.node.as_mut() else {
            return Err("SCOPE_UNRESOLVED");
        };
        if !col.columns.is_empty() {
            return Err("SCOPE_UNRESOLVED");
        }
        if let Some(path) = col.pathspec.as_mut() {
            walk_closed(path.string.as_deref_mut(), policy, ctes, visits)?;
        }
        if let Some(type_name) = col.type_name.as_mut() {
            for name_node in &mut type_name.names {
                walk_closed(Some(name_node), policy, ctes, visits)?;
            }
            for typmod in &mut type_name.typmods {
                walk_closed(Some(typmod), policy, ctes, visits)?;
            }
        }
        if col.name.is_empty() {
            return Err("SCOPE_UNRESOLVED");
        }
        cols.push(OutCol { qualifier: name.clone(), name: col.name.clone(), large_base: false });
    }
    if cols.is_empty() {
        return Err("SCOPE_UNRESOLVED");
    }
    Ok((named_item(name, cols), false))
}

fn source_from_function(
    func: &mut RangeFunction,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(FromItem, bool), &'static str> {
    if func.lateral || func.ordinality || func.is_rowsfrom {
        return Err("LATERAL_DEFERRED");
    }
    for node in &mut func.functions {
        walk_closed(Some(node), policy, ctes, visits)?;
    }
    let name = alias_name(func.alias.as_ref()).ok_or("SCOPE_UNRESOLVED")?;
    let mut cols = Vec::new();
    for column in &func.coldeflist {
        let Some(N::ColumnDef(def)) = column.node.as_ref() else {
            return Err("SCOPE_UNRESOLVED");
        };
        if def.colname.is_empty() {
            return Err("SCOPE_UNRESOLVED");
        }
        cols.push(OutCol { qualifier: name.clone(), name: def.colname.clone(), large_base: false });
    }
    if cols.is_empty() {
        cols = alias_colnames(func.alias.as_ref())
            .into_iter()
            .map(|col| OutCol { qualifier: name.clone(), name: col, large_base: false })
            .collect();
    }
    if cols.is_empty() {
        return Err("SCOPE_UNRESOLVED");
    }
    Ok((named_item(name, cols), false))
}

fn walk_closed(
    node: Option<&mut Node>,
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(), &'static str> {
    let Some(node) = node else {
        return Ok(());
    };
    match walk_node(node, &[], policy, ctes, visits) {
        Err("COLUMN_NOT_ALLOWED" | "AMBIGUOUS_SCOPE" | "WHOLE_ROW_DEFERRED" | "STAR_SCOPE_DENIED") => Err("LATERAL_DEFERRED"),
        other => other,
    }
}

fn rewrite_targets(
    targets: &[Node],
    items: &[FromItem],
    scopes: &[ScopeView],
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(Vec<Node>, bool, bool), &'static str> {
    let mut out = Vec::new();
    let mut saw_star = false;
    let mut touches_large = false;
    for target in targets {
        let mut owned = target.clone();
        let star_parts = match owned.node.as_ref() {
            Some(N::ResTarget(res)) => {
                if !res.indirection.is_empty() {
                    return Err("SCOPE_UNRESOLVED");
                }
                res.val.as_deref().and_then(projection_star)
            }
            _ => None,
        };
        if let Some(parts) = star_parts {
            saw_star = true;
            let is_column_ref = matches!(
                owned.node.as_ref(),
                Some(N::ResTarget(res))
                    if res.val.as_deref().is_some_and(|val| matches!(val.node.as_ref(), Some(N::ColumnRef(_))))
            );
            if is_column_ref {
                *visits = visits.saturating_add(1);
            }
            let cols = star_cols(&parts, items)?;
            if cols.iter().any(|col| col.large_base) {
                touches_large = true;
            }
            for col in cols {
                out.push(res_target(&col.qualifier, &col.name));
            }
        } else {
            walk_node(&mut owned, scopes, policy, ctes, visits)?;
            out.push(owned);
        }
    }
    Ok((out, saw_star, touches_large))
}

fn star_cols(parts: &[String], items: &[FromItem]) -> Result<Vec<OutCol>, &'static str> {
    match parts {
        [star] if star == "*" => {
            let mut cols = Vec::new();
            if items.is_empty() {
                return Err("NO_VISIBLE_COLUMNS");
            }
            for item in items {
                if item.sources.iter().any(|source| source.cols.is_empty()) || item.stars.is_empty() {
                    return Err("NO_VISIBLE_COLUMNS");
                }
                cols.extend(item.stars.iter().cloned());
            }
            Ok(cols)
        }
        [qual, star] if star == "*" => {
            let matched: Vec<&Source> = items
                .iter()
                .flat_map(|item| item.sources.iter())
                .filter(|source| source.name == *qual)
                .collect();
            if matched.len() != 1 {
                return Err(if matched.is_empty() { "STAR_SCOPE_DENIED" } else { "AMBIGUOUS_SCOPE" });
            }
            if matched[0].cols.is_empty() {
                return Err("NO_VISIBLE_COLUMNS");
            }
            Ok(matched[0].cols.clone())
        }
        _ => Err("STAR_SCOPE_DENIED"),
    }
}

fn projection_star(node: &Node) -> Option<Vec<String>> {
    match node.node.as_ref() {
        Some(N::AStar(_)) => Some(vec!["*".to_string()]),
        Some(N::ColumnRef(col)) => {
            let parts = col_parts(col).ok()?;
            if parts.last().is_some_and(|part| part == "*") {
                Some(parts)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn output_names(targets: &[Node]) -> Result<Vec<String>, &'static str> {
    let mut names = Vec::new();
    for target in targets {
        let Some(N::ResTarget(res)) = target.node.as_ref() else {
            return Err("UNALIASED_EXPR");
        };
        if !res.name.is_empty() {
            names.push(res.name.clone());
            continue;
        }
        let Some(val) = res.val.as_deref() else {
            return Err("UNALIASED_EXPR");
        };
        let Some(N::ColumnRef(col)) = val.node.as_ref() else {
            return Err("UNALIASED_EXPR");
        };
        let parts = col_parts(col)?;
        let Some(name) = parts.last() else {
            return Err("UNALIASED_EXPR");
        };
        if name == "*" {
            return Err("STAR_STILL_PRESENT");
        }
        names.push(name.clone());
    }
    Ok(names)
}

fn walk_opt(
    node: Option<&mut Node>,
    scopes: &[ScopeView],
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(), &'static str> {
    if let Some(node) = node {
        walk_node(node, scopes, policy, ctes, visits)?;
    }
    Ok(())
}

fn walk_list(
    nodes: &mut [Node],
    scopes: &[ScopeView],
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(), &'static str> {
    for node in nodes {
        walk_node(node, scopes, policy, ctes, visits)?;
    }
    Ok(())
}

fn walk_node(
    node: &mut Node,
    scopes: &[ScopeView],
    policy: &Policy,
    ctes: &mut BTreeMap<String, Vec<OutCol>>,
    visits: &mut u32,
) -> Result<(), &'static str> {
    if node.node.is_none() {
        return Ok(());
    }
    match node.node.as_mut() {
        Some(N::ColumnRef(col)) => {
            *visits = visits.saturating_add(1);
            let parts = col_parts(col)?;
            if parts.iter().any(|part| part == "*") {
                return Err("STAR_SCOPE_DENIED");
            }
            resolve_col(&parts, scopes)
        }
        Some(N::AConst(_) | N::Integer(_) | N::Float(_) | N::Boolean(_) | N::String(_) | N::BitString(_) | N::ParamRef(_) | N::AStar(_)) => Ok(()),
        Some(N::AExpr(expr)) => {
            walk_list(&mut expr.name, scopes, policy, ctes, visits)?;
            walk_opt(expr.lexpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(expr.rexpr.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::BoolExpr(expr)) => {
            walk_opt(expr.xpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut expr.args, scopes, policy, ctes, visits)
        }
        Some(N::FuncCall(call)) => {
            walk_list(&mut call.funcname, scopes, policy, ctes, visits)?;
            walk_list(&mut call.args, scopes, policy, ctes, visits)?;
            walk_list(&mut call.agg_order, scopes, policy, ctes, visits)?;
            walk_opt(call.agg_filter.as_deref_mut(), scopes, policy, ctes, visits)?;
            if let Some(over) = call.over.as_mut() {
                walk_list(&mut over.partition_clause, scopes, policy, ctes, visits)?;
                walk_list(&mut over.order_clause, scopes, policy, ctes, visits)?;
                walk_opt(over.start_offset.as_deref_mut(), scopes, policy, ctes, visits)?;
                walk_opt(over.end_offset.as_deref_mut(), scopes, policy, ctes, visits)?;
            }
            Ok(())
        }
        Some(N::TypeCast(cast)) => {
            walk_opt(cast.arg.as_deref_mut(), scopes, policy, ctes, visits)?;
            if let Some(name) = cast.type_name.as_mut() {
                walk_list(&mut name.names, scopes, policy, ctes, visits)?;
                walk_list(&mut name.typmods, scopes, policy, ctes, visits)?;
                walk_list(&mut name.array_bounds, scopes, policy, ctes, visits)?;
            }
            Ok(())
        }
        Some(N::TypeName(name)) => {
            walk_list(&mut name.names, scopes, policy, ctes, visits)?;
            walk_list(&mut name.typmods, scopes, policy, ctes, visits)?;
            walk_list(&mut name.array_bounds, scopes, policy, ctes, visits)
        }
        Some(N::SubLink(link)) => {
            walk_opt(link.xpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(link.testexpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut link.oper_name, scopes, policy, ctes, visits)?;
            let Some(sub) = link.subselect.as_mut() else {
                return Err("UNKNOWN_NODE");
            };
            let Some(N::SelectStmt(select)) = sub.node.as_mut() else {
                return Err("SCOPE_UNRESOLVED");
            };
            rewrite_select(select, policy, ctes, scopes, false, visits)?;
            Ok(())
        }
        Some(N::NullTest(test)) => walk_opt(test.arg.as_deref_mut(), scopes, policy, ctes, visits),
        Some(N::BooleanTest(test)) => walk_opt(test.arg.as_deref_mut(), scopes, policy, ctes, visits),
        Some(N::CaseExpr(expr)) => {
            walk_opt(expr.xpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(expr.arg.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut expr.args, scopes, policy, ctes, visits)?;
            walk_opt(expr.defresult.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::CaseWhen(case)) => {
            walk_opt(case.xpr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(case.expr.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(case.result.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::CoalesceExpr(expr)) => walk_list(&mut expr.args, scopes, policy, ctes, visits),
        Some(N::SortBy(sort)) => {
            walk_opt(sort.node.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut sort.use_op, scopes, policy, ctes, visits)
        }
        Some(N::WindowDef(window)) => {
            walk_list(&mut window.partition_clause, scopes, policy, ctes, visits)?;
            walk_list(&mut window.order_clause, scopes, policy, ctes, visits)?;
            walk_opt(window.start_offset.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(window.end_offset.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::ResTarget(res)) => {
            if !res.indirection.is_empty() {
                return Err("SCOPE_UNRESOLVED");
            }
            walk_opt(res.val.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::GroupingSet(set)) => walk_list(&mut set.content, scopes, policy, ctes, visits),
        Some(N::List(list)) => walk_list(&mut list.items, scopes, policy, ctes, visits),
        Some(N::AArrayExpr(expr)) => walk_list(&mut expr.elements, scopes, policy, ctes, visits),
        Some(N::AIndices(idx)) => {
            walk_opt(idx.lidx.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_opt(idx.uidx.as_deref_mut(), scopes, policy, ctes, visits)
        }
        Some(N::AIndirection(ind)) => {
            walk_opt(ind.arg.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut ind.indirection, scopes, policy, ctes, visits)
        }
        Some(N::CollateClause(clause)) => {
            walk_opt(clause.arg.as_deref_mut(), scopes, policy, ctes, visits)?;
            walk_list(&mut clause.collname, scopes, policy, ctes, visits)
        }
        Some(other) => {
            if node_sensitive(other) {
                Err("UNKNOWN_NODE")
            } else {
                Ok(())
            }
        }
        None => Ok(()),
    }
}

fn node_sensitive(kind: &N) -> bool {
    let value = match serde_json::to_value(kind) {
        Ok(value) => value,
        Err(_) => return true,
    };
    let text = value.to_string();
    text.contains("\"ColumnRef\"")
        || text.contains("\"RangeVar\"")
        || text.contains("Stmt\"")
        || text.contains("\"IntoClause\"")
        || text.contains("\"LockingClause\"")
}

fn resolve_col(parts: &[String], scopes: &[ScopeView]) -> Result<(), &'static str> {
    match parts {
        [name] => {
            for scope in scopes {
                match hit_unqualified(name, scope) {
                    Hit::Missing => continue,
                    Hit::Allow => return Ok(()),
                    Hit::WholeRow => return Err("WHOLE_ROW_DEFERRED"),
                    Hit::Ambiguous => return Err("AMBIGUOUS_SCOPE"),
                }
            }
            Err("COLUMN_NOT_ALLOWED")
        }
        [qual, name] => {
            for scope in scopes {
                match hit_qualified(qual, name, scope) {
                    Qual::Missing => continue,
                    Qual::Allow => return Ok(()),
                    Qual::Ambiguous => return Err("AMBIGUOUS_SCOPE"),
                    Qual::Hidden => return Err("COLUMN_NOT_ALLOWED"),
                }
            }
            Err("SCOPE_UNRESOLVED")
        }
        _ => Err("SCOPE_UNRESOLVED"),
    }
}

enum Hit {
    Allow,
    WholeRow,
    Ambiguous,
    Missing,
}

enum Qual {
    Allow,
    Ambiguous,
    Hidden,
    Missing,
}

fn hit_unqualified(name: &str, scope: &ScopeView) -> Hit {
    let rel_hits = scope.sources.iter().filter(|source| source.name == *name).count();
    let col_hits = scope.outputs.iter().filter(|col| col.name == *name).count();
    if rel_hits > 0 && col_hits == 0 {
        return Hit::WholeRow;
    }
    if rel_hits > 0 && col_hits > 0 {
        return Hit::Ambiguous;
    }
    match col_hits {
        0 => Hit::Missing,
        1 => Hit::Allow,
        _ => Hit::Ambiguous,
    }
}

fn hit_qualified(qual: &str, name: &str, scope: &ScopeView) -> Qual {
    let matched: Vec<&Source> = scope.sources.iter().filter(|source| source.name == *qual).collect();
    match matched.len() {
        0 => Qual::Missing,
        1 if matched[0].cols.iter().any(|col| col.name == *name) => Qual::Allow,
        1 => Qual::Hidden,
        _ => Qual::Ambiguous,
    }
}

fn col_parts(col: &ColumnRef) -> Result<Vec<String>, &'static str> {
    let mut parts = Vec::new();
    for field in &col.fields {
        match field.node.as_ref() {
            Some(N::String(text)) => parts.push(text.sval.clone()),
            Some(N::AStar(_)) => parts.push("*".to_string()),
            _ => return Err("SCOPE_UNRESOLVED"),
        }
    }
    Ok(parts)
}

fn using_names(nodes: &[Node]) -> Result<Vec<String>, &'static str> {
    let mut names = Vec::new();
    for node in nodes {
        match node.node.as_ref() {
            Some(N::String(text)) => names.push(text.sval.clone()),
            _ => return Err("SCOPE_UNRESOLVED"),
        }
    }
    Ok(names)
}

fn alias_name(alias: Option<&pg_query::protobuf::Alias>) -> Option<String> {
    alias.map(|alias| alias.aliasname.clone()).filter(|name| !name.is_empty())
}

fn alias_colnames(alias: Option<&pg_query::protobuf::Alias>) -> Vec<String> {
    alias
        .map(|alias| {
            alias
                .colnames
                .iter()
                .filter_map(|node| match node.node.as_ref() {
                    Some(N::String(text)) => Some(text.sval.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_colnames(cols: &mut [OutCol], colnames: &[String]) -> Result<(), &'static str> {
    if colnames.is_empty() {
        return Ok(());
    }
    if colnames.len() != cols.len() {
        return Err("SCOPE_UNRESOLVED");
    }
    for (col, name) in cols.iter_mut().zip(colnames) {
        col.name = name.clone();
    }
    Ok(())
}

fn res_target(qualifier: &str, name: &str) -> Node {
    Node {
        node: Some(N::ResTarget(Box::new(ResTarget {
            name: String::new(),
            indirection: vec![],
            val: Some(Box::new(col_node(qualifier, name))),
            location: -1,
        }))),
    }
}

fn col_node(qualifier: &str, name: &str) -> Node {
    let mut fields = Vec::new();
    if !qualifier.is_empty() {
        fields.push(str_node(qualifier));
    }
    fields.push(str_node(name));
    Node {
        node: Some(N::ColumnRef(ColumnRef { fields, location: -1 })),
    }
}

fn str_node(text: &str) -> Node {
    Node {
        node: Some(N::String(pg_query::protobuf::String { sval: text.to_string() })),
    }
}
