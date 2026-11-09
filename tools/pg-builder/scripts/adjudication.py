"""D28 aggregate-only adjudication over an immutable Agent DB retained cut."""
import argparse
import hashlib
import json
from pathlib import Path
import re

import library_selfbuild as lib
from library_observation import quote, literal
import measure_reconciliation

CONFIG = Path(__file__).with_name("adjudication-thresholds.json")
CLASSES = {
    "relation": {"adopted", "rejected"}, "sensitive": {"hidden", "visible"},
    "metric": {"certified", "demoted"}, "entity_key": {"adopted", "rejected"},
    "code_table": {"complete", "incomplete"},
}
FORMAT_FIELDS = ("phone", "id_card", "email", "bank_card", "person_name",
                 "street_address", "contact_text", "birth_date", "pii_marker")


def semantic_predicates(column):
    # Aggregate on the replica: neither names nor contact samples leave it.
    surnames = ("赵钱孙李周吴郑王冯陈褚卫蒋沈韩杨朱秦尤许何吕施张孔曹严华金魏陶姜"
                "戚谢邹喻柏水窦章云苏潘葛奚范彭郎鲁韦昌马苗凤花方俞任袁柳鲍史唐费"
                "廉岑薛雷贺倪汤滕殷罗毕郝邬安常乐于傅皮卞齐康伍余元顾孟平黄穆萧尹"
                "姚邵汪毛禹狄米贝明臧计伏成戴宋茅庞熊纪舒屈项祝董梁杜阮蓝闵席季"
                "麻强贾路娄危江童颜郭梅盛林钟徐邱骆高夏蔡田樊胡凌霍虞万支柯管卢莫"
                "房裘缪解应宗丁宣邓郁单杭洪包诸左石崔吉龚程嵇邢裴陆荣翁荀羊甄曲"
                "封芮储靳段富巫乌焦巴弓牧隗山谷车侯全班仰秋仲伊宫宁仇栾暴甘厉戎"
                "祖武符刘景詹束龙叶幸司韶黎乔苍双闻莘党翟谭贡劳姬申冉宰雍桑桂牛"
                "寿通边扈燕冀郏浦尚农温庄晏柴瞿阎连习艾向古易慎廖庾终居衡步都耿"
                "满弘匡国文寇广禄阙东欧殳沃利蔚越夔隆师巩厍聂晁勾敖融冷訾辛阚"
                "简饶曾沙关蒯相查后荆红游竺权逯盖益桓公")
    cjk = rf"^([{surnames}][一-龥]{{1,2}}|(欧阳|司马|上官|诸葛|夏侯|东方|皇甫|尉迟|公孙|慕容)[一-龥]{{1,2}})$"
    given = ("Alice|Bob|Carol|David|Emma|Frank|Grace|Henry|Isabella|James|Jane|John|"
             "Julia|Liam|Maria|Mary|Michael|Olivia|Robert|Sarah|Sophia|Thomas|William")
    latin = rf"^({given})[[:space:]]+[A-Z][a-z]+([-'][A-Z]?[a-z]+)?$"
    address = (r"(^[0-9]{1,6}[[:space:]]+[[:alpha:] .'-]+[[:space:]]"
               r"(Street|St|Road|Rd|Avenue|Ave|Lane|Ln|Drive|Dr|Boulevard|Blvd)"
               r"([[:space:],.#]|$)|[一-龥]+(路|街|巷|弄|道)[0-9一二三四五六七八九十百]+号)")
    email = r"[[:alnum:]_.%+-]+@[[:alnum:].-]+\.[[:alpha:]]{2,}"
    contact = (r"(电话|手机|联系|tel|phone|mobile)[[:space:]:：=]*"
               r"\+?[0-9][0-9 ()-]{6,18}[0-9]")
    birth_context = bool(re.search(r"birth|birthday|dob|born|csrq|出生|生日",
                                   column.get("name", column["physical_name"]), re.I))
    date = r"^(19|20)[0-9]{2}[-/](0?[1-9]|1[0-2])[-/](0?[1-9]|[12][0-9]|3[01])$"
    return {
        "person_name": f"(v ~ {literal(cjk)} OR v ~ {literal(latin)}) "
                       "AND v !~ '(公司|集团|中心|银行|物流|客户|仓库|街|路|省|市|区|县|店)$'",
        "street_address": "v ~* " + literal(address),
        "contact_text": "(v ~* " + literal(email) + " OR v ~* " + literal(contact) + ")",
        "birth_date": ("v ~ " + literal(date) + " AND pg_input_is_valid(v,'date')"
                       if birth_context else "false"),
        # These are intentionally artificial fixture values, not evidence that
        # formatless names/addresses can be recognized by a canary prefix.
        "pii_marker": "v ~ '^(SYN-(PHONE|ID)-[0-9]+|CANARY-PII-(PHONE|SFZH)-[0-9]+)$'",
    }


def thresholds():
    return json.loads(CONFIG.read_text())


def summary_hash(document):
    return lib.digest({"graph": document["graph"],
                       "decisions": {i["id"]: i["decision"] for i in document["items"]}})


def column_profile(db, table, column):
    name = quote(column["physical_name"])
    semantic = "".join(
        f"'{field}',coalesce(sum(n) filter(where {predicate}),0),"
        for field, predicate in semantic_predicates(column).items())
    # Values never leave the replica; grouped frequencies provide Shannon entropy.
    return db.query(
        "select json_build_object('rows',coalesce(sum(n),0),"
        "'non_null',coalesce(sum(n) filter(where v is not null),0),"
        "'distinct',count(*) filter(where v is not null),"
        "'max_length',coalesce(max(length(v)),0),"
        "'readable',coalesce(sum(n) filter(where v ~ '[[:alpha:]]' "
        "and length(v) between 2 and 160),0),"
        "'phone',coalesce(sum(n) filter(where v ~ '^\\+?[0-9][0-9 ()-]{6,18}[0-9]$' "
        "and length(regexp_replace(v,'[^0-9]','','g')) between 10 and 15),0),"
        "'id_card',coalesce(sum(n) filter(where v ~ '^[1-9][0-9]{16}[0-9Xx]$'),0),"
        "'email',coalesce(sum(n) filter(where v ~ '^[^[:space:]@]+@[^[:space:]@]+\\.[^[:space:]@]+$'),0),"
        "'bank_card',coalesce(sum(n) filter(where v ~ '^[0-9]{13,19}$'),0),"
        + semantic +
        "'entropy',coalesce(-sum((n::numeric/nullif(total,0))*"
        "log(2,n::numeric/nullif(total,0))) filter(where v is not null),0)) "
        "from (select v,n,sum(n) filter(where v is not null) over() total from "
        "(select " + name + "::text v,count(*) n from " + table["physical"]
        + " group by " + name + "::text) counts) frequencies")


def sensitive_decision(profile, suspicious, config):
    rows, count = profile["rows"], profile["non_null"]
    if (any(type(profile[k]) is not int or profile[k] < 0
            for k in ("rows", "non_null", "distinct", "phone", "id_card", "email", "bank_card"))
            or not 0 <= profile["distinct"] <= count <= rows
            or any(type(profile.get(k, 0)) is not int or not 0 <= profile.get(k, 0) <= count
                   for k in FORMAT_FIELDS)):
        raise ValueError("adjudication_profile_counts")
    rate = max(profile.get(k, 0) / count if count else 0 for k in FORMAT_FIELDS)
    # An empty/all-null profile is unknown, not negative evidence.
    hidden = rate >= config["sensitive_format_rate"] or suspicious and not count
    return ("hidden" if hidden else "visible",
            "sensitive.format" if rate >= config["sensitive_format_rate"]
            else "sensitive.unmeasured" if hidden else "sensitive.negative_profile", rate)


def relation_decision(evidence, config):
    ratio = evidence["containment"]
    signal = (evidence["name_similarity"] >= config["relation_name_similarity"]
              or evidence["join_cooccurrence"] > 0)
    eligible = evidence["rows_checked"] > 0 and evidence["type_compatible"] and evidence["length_compatible"]
    strict = ratio >= config["relation_containment_strict"] and evidence["parent_unique"]
    supported = ratio >= config["relation_containment_with_signal"] and signal
    return ("adopted" if eligible and (strict or supported) else "rejected",
            "relation.containment_strict" if eligible and strict else
            "relation.containment_signal" if eligible and supported else "relation.insufficient")


def code_decision(rows, matched, code_rows, config):
    if any(type(n) is not int or n < 0 for n in (rows, matched, code_rows)) or matched > rows:
        raise ValueError("adjudication_code_counts")
    coverage = matched / rows if rows else 0
    complete = rows > 0 and coverage >= config["code_coverage"] and code_rows <= config["code_max_rows"]
    return "complete" if complete else "incomplete", coverage


def type_family(column):
    kind = column["data_type"].lower()
    if re.match(r"^(smallint|integer|bigint|numeric|decimal|int\b)", kind):
        return "number"
    if re.match(r"^(text|character|varchar|char\b)", kind):
        return "text"
    return kind


def qualified_joins(sql, pairs):
    """Recognize direct equalities only; ambiguous aliases never add evidence."""
    sql = re.sub(r"'(?:''|[^'])*'|/\*.*?\*/|--[^\n]*|\$([A-Za-z_]\w*|)\$.*?\$\1\$",
                 " ", sql, flags=re.S)
    sql = sql.replace('"', "")
    aliases = {}
    for table, alias in re.findall(
            r"\b(?:from|join)\s+([A-Za-z_]\w*\.[A-Za-z_]\w*)"
            r"(?:\s+(?:as\s+)?((?!(?:on|where|join|left|right|inner|outer|group)\b)[A-Za-z_]\w*))?",
            sql, re.I):
        for name in {alias, table.split(".")[-1]} - {""}:
            aliases.setdefault(name, set()).add(table)
    for alias, tables in sorted(aliases.items(), key=lambda p: -len(p[0])):
        if len(tables) == 1:
            sql = re.sub(r"(?<![\w.])" + re.escape(alias) + r"\.",
                         next(iter(tables)) + ".", sql)
    return all(re.search(r"(?<![\w.])" + re.escape(a) + r"\s*=\s*" + re.escape(b)
                         + r"(?![\w.])", sql, re.I)
               or re.search(r"(?<![\w.])" + re.escape(b) + r"\s*=\s*" + re.escape(a)
                            + r"(?![\w.])", sql, re.I) for a, b in pairs)


def join_sources(db):
    definitions = db.query("select coalesce(json_agg(definition),'[]'::json) from pg_views "
                           "where schemaname not in ('pg_catalog','information_schema')")
    log = db.query(
        "select coalesce((select json_build_object('schema',n.nspname,'readable',"
        "has_table_privilege(c.oid,'SELECT')) from pg_extension e join pg_namespace n "
        "on n.oid=e.extnamespace join pg_class c on c.relnamespace=n.oid "
        "and c.relname='pg_stat_statements' where e.extname='pg_stat_statements'),'null'::json)")
    status = "unavailable"
    if log and log["readable"]:
        try:
            definitions += db.query(
                "select coalesce(json_agg(query),'[]'::json) from (select query from "
                + quote(log["schema"]) + '.pg_stat_statements where dbid='
                "(select oid from pg_database where datname=current_database()) "
                "order by query limit 10000) statements")
            status = "bounded_visible_pg_stat_statements"
        except RuntimeError:
            # Optional query history can be installed but not preloaded. Do not
            # call that empty evidence, or discard the independent view evidence.
            status = "query_failed"
    return definitions, status


def join_count(definitions, child, columns, parent, keys):
    pairs = [(child["dataset"] + "." + c["name"], parent["dataset"] + "." + k["name"])
             for c, k in zip(columns, keys)]
    return sum(qualified_joins(sql, pairs) for sql in definitions)


def discover_candidates(command, tables):
    """Expand before binding input bytes; adjudication measures every candidate."""
    edges = command["draft"]["graph"]["edges"]
    def identity(edge):
        return (edge["from"], tuple(edge["columns"]["from"]),
                edge["to"], tuple(edge["columns"]["to"]))
    seen = {identity(e) for e in edges}
    def keylike(table, name):
        return (name in table.get("primary_key", []) or
                bool(re.search(r"(^id$|id$|code$|_dm$|_bh$|_ddh$|ydh$)", name, re.I)))
    for child in sorted(tables, key=lambda t: t["dataset"]):
        for parent in sorted(tables, key=lambda t: t["dataset"]):
            if child["dataset"] == parent["dataset"]:
                continue
            pairs = [([c["name"]], [k["name"]])
                     for c in child["columns"] if keylike(child, c["name"])
                     for k in parent["columns"] if keylike(parent, k["name"])
                     and type_family(c) == type_family(k)]
            pk = parent.get("primary_key", [])
            if len(pk) > 1 and set(pk) <= {c["name"] for c in child["columns"]}:
                pairs.append((pk, pk))
            for columns, keys in pairs:
                edge = dict(**{"from": child["dataset"], "to": parent["dataset"]},
                            columns={"from": columns, "to": keys},
                            provenance="containment", confidence=0.5)
                signature = identity(edge)
                if signature not in seen:
                    seen.add(signature)
                    edge["id"] = "discovered-" + lib.digest(signature)[:24]
                    edges.append(edge)
    command["source_draft_sha256"] = lib.digest(command["draft"])


def collect(db, command, tables, input_sha256):
    config = thresholds()
    inventory = {t["dataset"]: t for t in tables}
    expected = {t["dataset"] for t in command["effective"]["datasets"]}
    if set(inventory) != expected:
        raise ValueError("adjudication_inventory")
    items, entities, edges, profiles = [], [], [], {}
    labels = {e["main_table"]: e["canonical_name"]
              for e in command["draft"].get("registry", {}).get("entities", {}).values()}

    def add(cls, identity, subject, decision, evidence, score, rule):
        item = dict(id=cls + "/" + identity, **{"class": cls}, subject=subject,
                    decision=decision, evidence=evidence, scores={"confidence": score},
                    rule_id=rule, reason=rule.replace(".", ": ").replace("_", " "))
        items.append(item)
        return item["id"]

    for dataset, table in sorted(inventory.items()):
        columns = {c["name"]: c for c in table["columns"]}
        for name, column in sorted(columns.items()):
            profile = column_profile(db, table, column)
            profiles[dataset, name] = profile
            suspicious = bool(re.search(r"phone|mobile|email|mail|sfzh|id_card|national_id|bank_card",
                                        name, re.I))
            decision, rule, rate = sensitive_decision(profile, suspicious, config)
            add("sensitive", dataset + "." + name, {"column": dataset + "." + name},
                decision, dict(profile, name_suspicion=suspicious, format_hit_rate=rate),
                rate if decision == "hidden" else 1 - rate, rule)
        key = table.get("primary_key", [])
        if not key:
            key = next(([n] for n in sorted(columns)
                        if profiles[dataset, n]["rows"] > 0 and
                        profiles[dataset, n]["distinct"] == profiles[dataset, n]["rows"]), [])
        rows = next(iter(profiles[dataset, n]["rows"] for n in columns), 0)
        measured = {"rows": rows, "non_null": 0, "distinct": 0}
        if key:
            names = [quote(columns[n]["physical_name"]) for n in key]
            measured = db.query(
                "select json_build_object('rows',count(*),'non_null',count(*) filter(where "
                + " and ".join(n + " is not null" for n in names)
                + "),'distinct',count(distinct ROW(" + ",".join(names) + "))) from " + table["physical"])
        valid = measured["rows"] > 0 and measured["rows"] == measured["non_null"] == measured["distinct"]
        readable = []
        for name, column in columns.items():
            p = profiles[dataset, name]
            visible = next(i["decision"] == "visible" for i in items
                           if i["id"] == "sensitive/" + dataset + "." + name)
            if (visible and type_family(column) == "text" and p["non_null"] and
                    p["distinct"] / max(rows, 1) >= config["display_distinct_ratio"]):
                readable.append((p["readable"] / max(rows, 1), name))
        display = max(readable)[1] if readable and max(readable)[0] > 0 else None
        evidence = dict(measured, distinct_ratio=measured["distinct"] / max(rows, 1),
                        non_null_ratio=measured["non_null"] / max(rows, 1),
                        key=key, display=display, full_scan=True)
        add("entity_key", dataset, {"table": dataset}, "adopted" if valid else "rejected",
            evidence, 1.0 if valid else 0.0, "entity_key.full_unique" if valid else "entity_key.dataset")
        entities.append(dict(id="entity/" + dataset, label=labels.get(dataset, dataset), table=dataset,
                             key=key if valid else [], display=display if valid else None, rows=rows))

    containment = lib.database_containment(db)
    definitions, log_status = join_sources(db) if command["draft"]["graph"]["edges"] else ([], "unavailable")
    for edge in command["draft"]["graph"]["edges"]:
        child, parent = inventory[edge["from"]], inventory[edge["to"]]
        columns = [next(c for c in child["columns"] if c["name"] == n) for n in edge["columns"]["from"]]
        keys = [next(c for c in parent["columns"] if c["name"] == n) for n in edge["columns"]["to"]]
        compatible = len(columns) == len(keys) and bool(columns) and all(
            type_family(c) == type_family(k) for c, k in zip(columns, keys))
        if compatible:
            observed = containment(child, columns if len(columns) > 1 else columns[0],
                                   parent, keys if len(keys) > 1 else keys[0])
            lib.validate_containment(observed)
        else:
            observed = dict(non_null=0, matched=0, parent_unique=False)
        count, matched = observed.get("non_null_rows", 0), observed.get("matched_rows", 0)
        ratio = matched / count if count else 0
        child_unique = observed["non_null"] == count and count > 0
        parent_unique = observed["parent_unique"]
        cardinality = ("one" if child_unique else "many") + "_to_" + ("one" if parent_unique else "many")
        length_ok = all(type_family(k) != "text" or
                        profiles[child["dataset"], c["name"]]["non_null"] == 0 or
                        not re.search(r"\((\d+)\)", k["data_type"]) or
                        profiles[child["dataset"], c["name"]]["max_length"] <=
                        int(re.search(r"\((\d+)\)", k["data_type"])[1])
                        for c, k in zip(columns, keys))
        similarity = sum(c["name"] == k["name"] or bool(lib.candidate_signal(child, c, parent, k))
                         for c, k in zip(columns, keys)) / max(len(columns), 1)
        evidence = dict(containment=ratio, parent_unique=parent_unique, type_compatible=compatible,
                        length_compatible=length_ok, name_similarity=similarity,
                        join_cooccurrence=join_count(definitions, child, columns, parent, keys),
                        cardinality=cardinality, rows_checked=count, child_distinct=observed["non_null"],
                        method="full_scan", query_log=log_status,
                        from_columns=edge["columns"]["from"], to_columns=edge["columns"]["to"])
        decision, rule = relation_decision(evidence, config)
        identity = add("relation", edge["id"], {
            "from": edge["from"] + "." + columns[0]["name"],
            "to": edge["to"] + "." + keys[0]["name"]},
                       decision, evidence, ratio, rule)
        edges.append(dict(relation=identity, **{"from": "entity/" + edge["from"],
                                              "to": "entity/" + edge["to"]},
                          cardinality=cardinality, adopted=decision == "adopted"))
        if (parent["primary_key"] == edge["columns"]["to"] and
                any("code" in n for n in edge["columns"]["to"]) and
                profiles[parent["dataset"], keys[0]["name"]]["rows"] <= config["code_max_rows"]):
            code, _ = code_decision(count, matched, profiles[parent["dataset"], keys[0]["name"]]["rows"], config)
            add("code_table", edge["id"], {"from": edge["from"], "to": edge["to"]}, code,
                dict(coverage=ratio, rows_checked=count), ratio, "code_table.coverage")

    # Unknown model SQL is never executed. Only a bound supported reconciliation
    # definition can certify it, including independently measured time windows.
    for spec in command.get("adjudication_specs", {}).get("code_tables", []):
        child, parent = inventory[spec["from"]["dataset"]], inventory[spec["to"]["dataset"]]
        cols = {c["name"]: c["physical_name"] for c in child["columns"]}
        keys = {c["name"]: c["physical_name"] for c in parent["columns"]}
        column, key = quote(cols[spec["from"]["column"]]), quote(keys[spec["to"]["column"]])
        scope = spec.get("scope", {})
        predicate = " and ".join(quote(keys[k]) + "::text=" + literal(v)
                                  for k, v in sorted(scope.items())) or "true"
        observed = db.query(
            "select json_build_object('rows_checked',count(*),'matched',"
            "count(*) filter(where exists(select 1 from " + parent["physical"] + " p where "
            + predicate + " and p." + key + "::text=c." + column + "::text)),"
            "'code_rows',(select count(*) from " + parent["physical"] + " where " + predicate
            + ")) from " + child["physical"] + " c where c." + column + " is not null")
        count = observed["rows_checked"]
        decision, coverage = code_decision(count, observed["matched"], observed["code_rows"], config)
        add("code_table", spec["id"],
            {"from":child["dataset"]+"."+spec["from"]["column"],
             "to":parent["dataset"]+"."+spec["to"]["column"]},
            decision,
            dict(observed, coverage=coverage, scope_sha256=lib.digest(scope)), coverage, "code_table.coverage")

    specs = command.get("adjudication_specs", {}).get("metrics", {})
    for metric in command["effective"].get("metrics", []):
        evidence = dict(non_empty=False, unit_matches=False, unique_keys=False,
                        rowwise_matches=False, stable_windows=False)
        spec = specs.get(metric["metric"])
        if spec:
            facts = {"tables": tables, "facts_sha256": lib.digest(tables)}
            try:
                logical, _ = measure_reconciliation.statements(facts, spec["measure"])
                for period in spec.get("windows", []):
                    measure_reconciliation.statements(facts, dict(spec["measure"], period=period))
            except (ValueError, KeyError, TypeError):
                logical = None
                evidence["definition_supported"] = False
            if logical is not None and metric["sql"] == logical and spec["measure"]["metric"] == metric["metric"]:
                reports = [measure_reconciliation.reconcile(db, facts, spec["measure"])]
                for period in spec.get("windows", []):
                    reports.append(measure_reconciliation.reconcile(
                        db, facts, dict(spec["measure"], period=period)))
                columns = {c["name"]: c for t in tables if t["dataset"] == spec["measure"]["dataset"]
                           for c in t["columns"]}
                units = {columns[c["column"]].get("unit") for c in spec["measure"].get("components", [])}
                unit_matches = (metric.get("unit") == "count" if spec["measure"]["kind"] == "count_distinct"
                                else units == {metric.get("unit")} and None not in units)
                evidence = dict(non_empty=all(r["result"]["records"] > 0 for r in reports),
                                unit_matches=unit_matches,
                                unique_keys=all(r["key_unique_in_scope"] for r in reports),
                                rowwise_matches=all(r["arithmetic_matches"] for r in reports),
                                stable_windows=len(reports) - 1 >= config["metric_min_windows"] and
                                all(a["end"] <= b["start"] for a, b in zip(
                                    sorted(spec.get("windows", []), key=lambda p: p["start"]),
                                    sorted(spec.get("windows", []), key=lambda p: p["start"])[1:])),
                                reconciliation_sha256=[r["evidence_sha256"] for r in reports])
        incomplete = {i["id"] for i in items
                      if i["class"] == "code_table" and i["decision"] == "incomplete"}
        dependencies = set((spec or {}).get("code_dependencies", []))
        known = {i["id"] for i in items if i["class"] == "code_table"}
        evidence["code_dependencies"] = sorted(dependencies)
        evidence["code_dependencies_complete"] = dependencies <= known and not incomplete.intersection(dependencies)
        passed = all(evidence[k] for k in ("non_empty", "unit_matches", "unique_keys",
                                          "rowwise_matches", "stable_windows", "code_dependencies_complete"))
        add("metric", metric["metric"], {"metric": metric["metric"]},
            "certified" if passed else "demoted", evidence, 1.0 if passed else 0.0, "metric.reconcile")
    document = dict(protocol="oo.adjudication/v1", input_sha256=input_sha256, thresholds=config,
                    items=sorted(items, key=lambda i: i["id"]),
                    graph=dict(entities=entities, edges=sorted(edges, key=lambda e: e["relation"])))
    if len({i["id"] for i in items}) != len(items):
        raise ValueError("adjudication_duplicate_id")
    document["summary_sha256"] = summary_hash(document)
    return document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    raw = args.input.read_bytes()
    command = json.loads(raw)
    import retained_build
    db = lib.ReadOnlyPostgres(config["psql_command"], config.get("statement_timeout_ms", 120000))
    facts, before = retained_build.facts_for(db, config)
    if command.get("adjudication_replica") != before:
        raise ValueError("adjudication_replica_binding")
    result = collect(db, command, facts["tables"], hashlib.sha256(raw).hexdigest())
    _, after = retained_build.facts_for(db, config)
    if before != after:
        raise ValueError("adjudication_retained_cut_changed")
    retained_build.save(args.input.with_name("adjudication.json"), result)


if __name__ == "__main__":
    main()
