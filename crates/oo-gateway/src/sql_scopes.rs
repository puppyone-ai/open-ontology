//! SELECT scopes share only the role catalog, never an unfiltered relation.
use super::*;

pub(super) struct Prepared {
    pub catalog: PolicyCatalog,
    pub nested: Vec<Admitted>,
}

pub(super) fn cartesian_input(node: &pb::Node) -> bool {
    match node.node.as_ref() {
        Some(pb::node::Node::RangeVar(_) | pb::node::Node::RangeSubselect(_)) => true,
        Some(pb::node::Node::JoinExpr(join)) => {
            join.jointype == JOIN_INNER
                && join.quals.is_none()
                && join.using_clause.is_empty()
                && !join.is_natural
                && join.larg.as_deref().is_some_and(cartesian_input)
                && join.rarg.as_deref().is_some_and(cartesian_input)
        }
        _ => false,
    }
}

pub(super) fn admit_union(
    query: &mut pb::SelectStmt,
    catalog: &PolicyCatalog,
    surface: Surface,
    outer: &[Vec<RangeBind>],
) -> Result<Admitted, GuardError> {
    if query.with_clause.is_some()
        || query.into_clause.is_some()
        || !query.locking_clause.is_empty()
        || !query.target_list.is_empty()
        || !query.from_clause.is_empty()
        || query.where_clause.is_some()
        || !query.group_clause.is_empty()
        || query.having_clause.is_some()
        || !query.distinct_clause.is_empty()
        || !query.window_clause.is_empty()
        || !query.values_lists.is_empty()
        || query.group_distinct
        || query.limit_option == LIMIT_TIES
    {
        return Err(unsupported("set operation"));
    }
    for node in [&query.limit_count, &query.limit_offset]
        .into_iter()
        .flatten()
    {
        nonnegative_const(node)?;
    }
    let mut left = admit_query(
        query
            .larg
            .as_deref_mut()
            .ok_or_else(|| unsupported("set operation"))?,
        catalog,
        surface,
        outer,
    )?;
    let right = admit_query(
        query
            .rarg
            .as_deref_mut()
            .ok_or_else(|| unsupported("set operation"))?,
        catalog,
        surface,
        outer,
    )?;
    if left.output_columns.len() != right.output_columns.len()
        || left
            .output_columns
            .iter()
            .zip(&right.output_columns)
            .any(|(a, b)| a.type_oid == 0 || a.type_oid != b.type_oid)
    {
        return Err(unsupported("set operation"));
    }
    for sort in &query.sort_clause {
        let Some(pb::node::Node::SortBy(sort)) = sort.node.as_ref() else {
            return Err(unsupported("order key"));
        };
        if sort.sortby_dir >= SORT_USING
            || !sort.use_op.is_empty()
            || sort.sortby_nulls > pb::SortByNulls::SortbyNullsLast as i32
        {
            return Err(unsupported("order key"));
        }
        let Some(pb::node::Node::AConst(value)) =
            sort.node.as_deref().and_then(|node| node.node.as_ref())
        else {
            return Err(unsupported("order key"));
        };
        if !matches!(&value.val, Some(pb::a_const::Val::Ival(i)) if !value.isnull && i.ival > 0 && i.ival as usize <= left.output_columns.len())
        {
            return Err(unsupported("order key"));
        }
    }
    left.estimated_rows = left.estimated_rows.saturating_add(right.estimated_rows);
    left.relations.extend(right.relations);
    left.functions.extend(right.functions);
    left.operators.extend(right.operators);
    left.expansions.extend(right.expansions);
    left.null_tests = left.null_tests.saturating_add(right.null_tests);
    left.starred.clear();
    Ok(left)
}

fn select(node: &mut pb::Node) -> Result<&mut pb::SelectStmt, GuardError> {
    match node.node.as_mut() {
        Some(pb::node::Node::SelectStmt(query)) => Ok(query),
        _ => Err(GuardError::Statement),
    }
}

pub(super) fn prepare(
    query: &mut pb::SelectStmt,
    catalog: &PolicyCatalog,
    surface: Surface,
    outer: &[Vec<RangeBind>],
) -> Result<Prepared, GuardError> {
    let mut prepared = Prepared {
        catalog: catalog.clone(),
        nested: Vec::new(),
    };
    if let Some(with) = query.with_clause.as_mut() {
        if with.recursive {
            return Err(unsupported("CTE"));
        }
        let mut names = BTreeSet::new();
        for node in &mut with.ctes {
            let Some(pb::node::Node::CommonTableExpr(cte)) = node.node.as_mut() else {
                return Err(unsupported("CTE"));
            };
            if !closed_ident(&cte.ctename)
                || !names.insert(cte.ctename.clone())
                || cte.search_clause.is_some()
                || cte.cycle_clause.is_some()
            {
                return Err(unsupported("CTE"));
            }
            let query = select(
                cte.ctequery
                    .as_deref_mut()
                    .ok_or_else(|| unsupported("CTE"))?,
            )?;
            let admitted = admit_query(query, &prepared.catalog, surface, outer)?;
            let columns = renamed_columns(&admitted.output_columns, &cte.aliascolnames)?;
            add_relation(
                &mut prepared.catalog,
                "",
                &cte.ctename,
                columns,
                admitted.estimated_rows,
            );
            prepared.nested.push(admitted);
        }
    }
    for node in &mut query.from_clause {
        prepare_from(node, &mut prepared, surface, outer)?;
    }
    Ok(prepared)
}

fn prepare_from(
    node: &mut pb::Node,
    prepared: &mut Prepared,
    surface: Surface,
    outer: &[Vec<RangeBind>],
) -> Result<(), GuardError> {
    match node.node.as_mut() {
        Some(pb::node::Node::JoinExpr(join)) => {
            for child in [&mut join.larg, &mut join.rarg].into_iter().flatten() {
                prepare_from(child, prepared, surface, outer)?;
            }
        }
        Some(pb::node::Node::RangeSubselect(sub)) => {
            if sub.lateral {
                return Err(unsupported("subquery"));
            }
            let alias = sub.alias.as_ref().ok_or_else(|| unsupported("subquery"))?;
            if !closed_ident(&alias.aliasname) {
                return Err(unsupported("identifier"));
            }
            let query = select(
                sub.subquery
                    .as_deref_mut()
                    .ok_or_else(|| unsupported("subquery"))?,
            )?;
            let admitted = admit_query(query, &prepared.catalog, surface, outer)?;
            let columns = renamed_columns(&admitted.output_columns, &alias.colnames)?;
            add_relation(
                &mut prepared.catalog,
                "@derived",
                &alias.aliasname,
                columns,
                admitted.estimated_rows,
            );
            prepared.nested.push(admitted);
        }
        _ => {}
    }
    Ok(())
}

fn renamed_columns(
    columns: &[VisibleColumn],
    names: &[pb::Node],
) -> Result<Vec<VisibleColumn>, GuardError> {
    let names = string_fields(names)?;
    if names.len() > columns.len() {
        return Err(unsupported("column alias list"));
    }
    let mut columns = columns.to_vec();
    let mut seen = BTreeSet::new();
    for (index, column) in columns.iter_mut().enumerate() {
        if let Some(name) = names.get(index) {
            column.name = name.clone();
        }
        if column.type_oid == 0
            || !closed_logical_ident(&column.name)
            || !seen.insert(column.name.clone())
        {
            return Err(unsupported("output alias"));
        }
        column.physical_name = column.name.clone();
    }
    Ok(columns)
}

fn add_relation(
    catalog: &mut PolicyCatalog,
    schema: &str,
    name: &str,
    columns: Vec<VisibleColumn>,
    estimated_rows: u64,
) {
    catalog.relations.push(VisibleRelation {
        schema: schema.into(),
        name: name.into(),
        physical_schema: String::new(),
        physical_name: name.into(),
        columns,
        pending_unclassified: 0,
        estimated_rows,
    });
}

fn bind_virtual(
    schema: &str,
    name: &str,
    alias: Option<String>,
    ctx: &mut Ctx<'_>,
) -> Result<bool, GuardError> {
    let Some(rel_index) = ctx
        .catalog
        .relations
        .iter()
        .rposition(|r| r.schema == schema && r.name == name)
    else {
        return Ok(false);
    };
    let exposed = alias.clone().unwrap_or_else(|| name.into());
    if ctx
        .ranges
        .iter()
        .any(|range| conflict_name(range, ctx.catalog) == exposed)
    {
        return Err(unsupported("duplicate range name"));
    }
    ctx.ranges.push(RangeBind {
        rel_index,
        alias,
        exposed,
        ordinal: ctx.next_ordinal,
    });
    ctx.next_ordinal += 1;
    Ok(true)
}

pub(super) fn bind_cte(var: &pb::RangeVar, ctx: &mut Ctx<'_>) -> Result<bool, GuardError> {
    bind_virtual("", &var.relname, range_alias(var)?, ctx)
}

pub(super) fn bind_derived(sub: &pb::RangeSubselect, ctx: &mut Ctx<'_>) -> Result<(), GuardError> {
    let alias = sub.alias.as_ref().ok_or_else(|| unsupported("subquery"))?;
    if !bind_virtual(
        "@derived",
        &alias.aliasname,
        Some(alias.aliasname.clone()),
        ctx,
    )? {
        return Err(unsupported("subquery"));
    }
    Ok(())
}

pub(super) fn resolve_column_scope(
    names: &[String],
    ctx: &Ctx<'_>,
) -> Result<(RangeBind, usize), GuardError> {
    let local_error = match resolve_column(names, ctx) {
        Ok((slot, column)) => return Ok((ctx.ranges[slot].clone(), column)),
        Err(error) => error,
    };
    // Only explicit correlations are admitted. With no hidden-name inventory,
    // falling through an unqualified name could bind a different outer column.
    if names.len() < 2 {
        return Err(local_error);
    }
    let qualifier = &names[..names.len() - 1];
    if !qualifier_slots(qualifier, ctx).is_empty() {
        return Err(local_error);
    }
    for scope in ctx.outer_ranges.iter().rev() {
        let matches: Vec<_> = scope
            .iter()
            .filter(|range| qualifier_matches(range, ctx.catalog, qualifier))
            .collect();
        match matches.as_slice() {
            [] => continue,
            [range] => {
                return column_index(ctx.catalog, range.rel_index, names.last().unwrap())
                    .map(|column| ((*range).clone(), column))
                    .ok_or(local_error);
            }
            _ => return Err(local_error),
        }
    }
    Err(local_error)
}

pub(super) fn admit_link(
    link: &mut pb::SubLink,
    ctx: &mut Ctx<'_>,
) -> Result<ExprKind, GuardError> {
    let kind =
        pb::SubLinkType::try_from(link.sub_link_type).map_err(|_| unsupported("subquery"))?;
    if !matches!(
        kind,
        pb::SubLinkType::ExprSublink | pb::SubLinkType::ExistsSublink | pb::SubLinkType::AnySublink
    ) {
        return Err(unsupported("subquery"));
    }
    let left = if kind == pb::SubLinkType::AnySublink {
        if !link.oper_name.is_empty() && string_fields(&link.oper_name)? != ["="] {
            return Err(unsupported("subquery"));
        }
        let test = link
            .testexpr
            .as_deref_mut()
            .ok_or_else(|| unsupported("subquery"))?;
        let kind = admit_expr(test, ctx)?;
        Some(infer_type(&kind, ctx.catalog).ok_or(GuardError::FunctionNotAllowed)?)
    } else {
        if link.testexpr.is_some() || !link.oper_name.is_empty() {
            return Err(unsupported("subquery"));
        }
        None
    };
    let mut outer = ctx.outer_ranges.clone();
    outer.push(ctx.ranges.clone());
    let query = select(
        link.subselect
            .as_deref_mut()
            .ok_or_else(|| unsupported("subquery"))?,
    )?;
    let admitted = admit_query(query, ctx.catalog, ctx.surface, &outer)?;
    let result = if kind == pb::SubLinkType::ExistsSublink {
        Type::BOOL.oid()
    } else {
        let [column] = admitted.output_columns.as_slice() else {
            return Err(unsupported("subquery"));
        };
        if column.type_oid == 0 {
            return Err(unsupported("subquery"));
        }
        if let Some(left) = left {
            let op = resolve_binary_operator(
                ctx.catalog,
                Some("pg_catalog"),
                "=",
                left,
                column.type_oid,
                false,
                false,
            )?;
            ctx.operators.push(BoundOperator {
                oid: op.oid,
                procedure_oid: op.procedure_oid,
                schema: op.schema.clone(),
                name: op.name.clone(),
            });
            Type::BOOL.oid()
        } else {
            column.type_oid
        }
    };
    ctx.nested.push(admitted);
    Ok(ExprKind::Case {
        result_type_oid: result,
    })
}

pub(super) fn output_column(target: &pb::Node, type_oid: u32) -> Result<VisibleColumn, GuardError> {
    let Some(pb::node::Node::ResTarget(target)) = target.node.as_ref() else {
        return Err(unsupported("target"));
    };
    let name = if target.name.is_empty() {
        output_name(target.val.as_deref().ok_or_else(|| unsupported("target"))?)?
    } else {
        target.name.clone()
    };
    Ok(VisibleColumn {
        physical_name: name.clone(),
        name,
        type_oid,
    })
}

fn output_name(node: &pb::Node) -> Result<String, GuardError> {
    Ok(match node.node.as_ref() {
        Some(pb::node::Node::ColumnRef(column)) => {
            plain_names(column)?.last().cloned().unwrap_or_default()
        }
        Some(pb::node::Node::FuncCall(call)) => function_name(&call.funcname)?.1,
        Some(pb::node::Node::CoalesceExpr(_)) => "coalesce".into(),
        Some(pb::node::Node::SqlvalueFunction(_)) => "current_date".into(),
        Some(pb::node::Node::AExpr(expr)) if expr.kind == pb::AExprKind::AexprNullif as i32 => {
            "nullif".into()
        }
        Some(pb::node::Node::SubLink(link))
            if link.sub_link_type == pb::SubLinkType::ExprSublink as i32 =>
        {
            match link
                .subselect
                .as_deref()
                .and_then(|node| node.node.as_ref())
            {
                Some(pb::node::Node::SelectStmt(query)) if query.target_list.len() == 1 => {
                    output_column(&query.target_list[0], 0)?.name
                }
                _ => "?column?".into(),
            }
        }
        Some(pb::node::Node::MinMaxExpr(expr)) => if expr.op == pb::MinMaxOp::IsGreatest as i32 {
            "greatest"
        } else {
            "least"
        }
        .into(),
        Some(pb::node::Node::CaseExpr(_)) => "case".into(),
        Some(pb::node::Node::TypeCast(cast)) => {
            let name = output_name(cast.arg.as_deref().ok_or_else(|| unsupported("cast"))?)?;
            if name != "?column?" {
                name
            } else {
                string_fields(
                    &cast
                        .type_name
                        .as_ref()
                        .ok_or_else(|| unsupported("cast"))?
                        .names,
                )?
                .last()
                .cloned()
                .unwrap_or_default()
            }
        }
        _ => "?column?".into(),
    })
}
