"""D28 aggregate-only adjudication over an immutable Agent DB retained cut."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re

import library_selfbuild as lib
from library_observation import quote, literal
import measure_reconciliation
from relation_keys import coverage_class, declared_uniques, key_family, keylike, observed_unique

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
    """Structure first: a JOIN that can multiply rows is never adopted."""
    ratio = evidence["containment"]
    distinct = evidence.get("distinct_containment", ratio)
    signal = (evidence["name_similarity"] >= config["relation_name_similarity"]
              or evidence["join_cooccurrence"] > 0 or evidence.get("kind") == "declared_fk")
    eligible = evidence["rows_checked"] > 0 and evidence["type_compatible"] and evidence["length_compatible"]
    if not eligible:
        return "rejected", "relation.insufficient"
    if not evidence["parent_unique"] or evidence.get("max_fanout", 1) > 1:
        return "rejected", ("relation.scope_required" if evidence.get("scope_columns")
                            else "relation.parent_not_unique")
    if evidence.get("normalized_collisions", 0) > 0:
        return "rejected", "relation.normalized_collision"
    if evidence.get("source") == "discovery" and not signal:
        # Open recall pairs every compatible key; containment alone between
        # small integer ranges is coincidence, not a reference.
        return "rejected", "relation.no_signal"
    strict = min(ratio, distinct) >= config["relation_containment_strict"]
    supported = min(ratio, distinct) >= config["relation_containment_with_signal"] and signal
    return ("adopted" if strict or supported else "rejected",
            "relation.containment_strict" if strict else
            "relation.containment_signal" if supported else "relation.insufficient")


def relation_reason(rule, evidence):
    """Short, fixed wording; the TUI owns translation by rule id."""
    if rule == "relation.scope_required":
        return ("Parent key repeats; unique only together with "
                + ", ".join(evidence["scope_columns"]) + ". Filter the parent on that column first.")
    if rule == "relation.parent_not_unique":
        return "Parent key repeats; a join would multiply child rows."
    if rule == "relation.normalized_collision":
        return "Parent keys collide after trim/lower-case; the match is ambiguous."
    if rule == "relation.no_signal":
        return "Values fit, but no name, declaration or query links these columns."
    return rule.replace(".", ": ").replace("_", " ")


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


ROLE_SUFFIX = re.compile(r"(_?(id|uuid|guid|code|cd|no|num|nbr|key|dm|bh|ddh)|Id|ID|Code|No|Key)$")
SCOPE_NAME = re.compile(r"(date|period|month|year|version|(^|_)ver$|^valid_|_valid_|effective|"
                        r"snapshot|as_of|(^|_)dt$|(^|_)ym$|期间|版本|日期|月份|年度)", re.I)
KIND_ORDER = {"declared_fk": 0, "model": 1, "inferred_id": 2, "snapshot_name_mapping": 3}


def candidate_keys(table):
    """Registry of referenceable keys. Name-only text columns are mappings, never keys."""
    keys, mappings = [], []
    if table.get("primary_key"):
        keys.append((tuple(table["primary_key"]), "declared_pk"))
    keys += [(k, "declared_unique") for k in declared_uniques(table)]
    for column in sorted(table["columns"], key=lambda c: c["name"]):
        if (column["name"],) in {k for k, _ in keys} or not observed_unique(column):
            continue
        family = key_family(column)
        if family and keylike(column["name"]):
            keys.append(((column["name"],), "observed_unique"))
        elif family == "text":
            mappings.append(column["name"])
    return keys, mappings


def shape_compatible(child, parent):
    """Value-shape buckets: pair only where some value could match at all."""
    c, p = child.get("profile") or {}, parent.get("profile") or {}
    if not c.get("non_null") or not p.get("non_null"):
        return "profile" not in child or "profile" not in parent
    if c.get("min_length") is not None and p.get("min_length") is not None and (
            c["max_length"] < p["min_length"] or c["min_length"] > p["max_length"]):
        return False
    filled = c["non_null"] - c.get("blank_rows", 0)
    if p.get("digits_rows") == p["non_null"] and not c.get("digits_rows"):
        return False
    # Every child value has a leading zero and no parent value does: no match is possible.
    return not (filled and c.get("leading_zero_rows") == filled and not p.get("leading_zero_rows"))


def mapping_signal(child_column, parent, parent_column):
    """region_name -> regions.name: the stem names the parent table."""
    name, target = child_column.lower(), parent_column.lower()
    if not name.endswith("_" + target):
        return False
    stem = name[:-len(target) - 1]
    table = parent.get("oracle", {}).get("source_table", parent["dataset"].split(".")[-1]).lower()
    singular = {table, table.rstrip("s"), re.sub(r"ies$", "y", table), table.split("_")[-1].rstrip("s")}
    return any(t and (stem == t or stem.endswith("_" + t)) for t in singular)


def discover_candidates(command, tables):
    """Expand before binding input bytes; adjudication measures every candidate."""
    edges = command["draft"]["graph"]["edges"]
    def identity(edge):
        return (edge["from"], tuple(edge["columns"]["from"]),
                edge["to"], tuple(edge["columns"]["to"]))
    seen = {identity(e) for e in edges}
    def add(child, columns, parent, keys, provenance):
        edge = dict(**{"from": child["dataset"], "to": parent["dataset"]},
                    columns={"from": list(columns), "to": list(keys)},
                    provenance=provenance, confidence=0.5)
        signature = identity(edge)
        if signature not in seen:
            seen.add(signature)
            edge["id"] = "discovered-" + lib.digest(signature)[:24]
            edges.append(edge)
    inventory = {t["dataset"]: t for t in tables}
    ordered = sorted(tables, key=lambda t: t["dataset"])
    for child in ordered:
        for fk in child.get("foreign_keys", []):
            if fk.get("referenced_dataset") in inventory:
                add(child, fk["columns"], inventory[fk["referenced_dataset"]],
                    fk["referenced_columns"], "declared_fk")
    # Parent statistics come from the bound facts profile, computed once per key.
    registry = {t["dataset"]: candidate_keys(t) for t in ordered}
    for child in ordered:
        columns = {c["name"]: c for c in child["columns"]}
        for parent in ordered:
            if child["dataset"] == parent["dataset"]:
                continue
            parent_columns = {c["name"]: c for c in parent["columns"]}
            keys, mappings = registry[parent["dataset"]]
            for key, _source in keys:
                if len(key) > 1:
                    if set(key) <= set(columns) and all(
                            type_family(columns[k]) == type_family(parent_columns[k]) for k in key):
                        add(child, key, parent, key, "containment")
                    continue
                target = parent_columns[key[0]]
                for column in sorted(columns.values(), key=lambda c: c["name"]):
                    # A table's own generic identity column names itself, not a parent.
                    if column["name"] in child.get("primary_key", []) and \
                            column["name"].lower() in ("id", "uuid", "guid", "code", "key", "no"):
                        continue
                    if (keylike(column["name"]) and key_family(column) == key_family(target)
                            and type_family(column) == type_family(target)
                            and shape_compatible(column, target)):
                        add(child, [column["name"]], parent, key, "containment")
            for name in mappings:
                target = parent_columns[name]
                for column in sorted(columns.values(), key=lambda c: c["name"]):
                    if (type_family(column) == "text" and mapping_signal(column["name"], parent, name)
                            and shape_compatible(column, target)):
                        add(child, [column["name"]], parent, [name], "name_mapping")
    command["source_draft_sha256"] = lib.digest(command["draft"])


def edge_kind(edge, child):
    declared = any(fk.get("referenced_dataset") == edge["to"] and fk["columns"] == edge["columns"]["from"]
                   and fk["referenced_columns"] == edge["columns"]["to"]
                   for fk in child.get("foreign_keys", []))
    if declared or edge.get("provenance") in ("declared_fk", "declared"):
        return "declared_fk"
    if edge.get("provenance") == "name_mapping":
        return "snapshot_name_mapping"
    return "inferred_id"


def role_name(columns, targets=()):
    stems = []
    for column, target in zip(columns, list(targets) + [None] * len(columns)):
        if target and column.lower().endswith("_" + target.lower()) and len(column) > len(target) + 1:
            stems.append(column[:-len(target) - 1])
        else:
            stems.append(ROLE_SUFFIX.sub("", column).strip("_") or column)
    return re.sub(r"[^a-z0-9]+", "_", "_".join(stems).lower()).strip("_") or None


def measure_edge(db, child, columns, parent, keys, scope=()):
    """One aggregate per edge, inside the caller's bound retained cut.

    Values compare as text (no numeric cast drops a leading zero); NULL and
    blank keys are counted and excluded from matching on both sides.
    """
    def expr(cols, alias=""):
        names = [alias + quote(c["physical_name"]) + "::text" for c in cols]
        return names[0] if len(names) == 1 else "ROW(" + ",".join(names) + ")"
    def present(cols):
        return " and ".join(quote(c["physical_name"]) + " is not null" for c in cols)
    def blank(cols):
        return "(" + " or ".join("btrim(" + quote(c["physical_name"]) + "::text)=''" for c in cols) + ")"
    norm = ("ROW(" + ",".join("lower(btrim(" + quote(k["physical_name"]) + "::text))" for k in keys) + ")"
            if len(keys) > 1 else "lower(btrim(" + quote(keys[0]["physical_name"]) + "::text))")
    scoped = "".join(
        ",'" + s["name"].replace("'", "''") + "',(select count(*)>0 and count(*)=count(distinct ROW("
        + expr(keys) + "," + quote(s["physical_name"]) + "::text)) and count(*) filter(where "
        + quote(s["physical_name"]) + " is null)=0 from " + parent["physical"] + " where "
        + present(keys) + " and not " + blank(keys) + ")" for s in scope)
    # A scalar subquery keeps the statement a plain SELECT for the read-only client.
    return db.query(
        "select (with c as (select " + expr(columns) + " k,count(*) n from " + child["physical"]
        + " where " + present(columns) + " and not " + blank(columns) + " group by 1),"
        " p as (select " + expr(keys) + " k,count(*) n from " + parent["physical"]
        + " where " + present(keys) + " and not " + blank(keys) + " group by 1),"
        " m as (select c.n cn,p.n pn from c join p on p.k=c.k) "
        "select json_build_object('child_rows',(select count(*) from " + child["physical"]
        + " where " + present(columns) + "),'empty_rows',(select count(*) from " + child["physical"]
        + " where " + present(columns) + " and " + blank(columns) + "),"
        "'child_distinct',(select count(*) from c),'non_blank_rows',(select coalesce(sum(n),0) from c),"
        "'matched_distinct',(select count(*) from m),'matched_rows',(select coalesce(sum(cn),0) from m),"
        "'max_fanout',(select coalesce(max(pn),0) from m),"
        "'parent_rows',(select count(*) from " + parent["physical"] + " where " + present(keys) + "),"
        "'parent_empty_rows',(select count(*) from " + parent["physical"] + " where " + present(keys)
        + " and " + blank(keys) + "),'parent_distinct',(select count(*) from p),"
        "'parent_duplicate_keys',(select count(*) from p where n>1),"
        "'parent_normalized_distinct',(select count(distinct " + norm + ") from " + parent["physical"]
        + " where " + present(keys) + " and not " + blank(keys) + "),"
        "'scope',json_build_object(" + scoped[1:] + ")))")


MEASUREMENT_FIELDS = ("child_rows", "empty_rows", "child_distinct", "non_blank_rows", "matched_distinct",
                      "matched_rows", "max_fanout", "parent_rows", "parent_empty_rows", "parent_distinct",
                      "parent_duplicate_keys", "parent_normalized_distinct")


def validate_measurement(m):
    if (not isinstance(m, dict) or any(type(m.get(k)) is not int or m[k] < 0 for k in MEASUREMENT_FIELDS)
            or not isinstance(m.get("scope"), dict)
            or any(type(v) is not bool for v in m["scope"].values())
            or not (m["matched_distinct"] <= m["child_distinct"] <= m["non_blank_rows"]
                    and m["matched_rows"] <= m["non_blank_rows"]
                    and m["non_blank_rows"] + m["empty_rows"] == m["child_rows"]
                    and m["parent_distinct"] <= m["parent_rows"] - m["parent_empty_rows"]
                    and m["parent_normalized_distinct"] <= m["parent_distinct"])):
        raise ValueError("adjudication_measurement_counts")
    return m


class MeasurementCache:
    """Resume file beside the input. Entries bind the facts digest and the edge."""
    PROTOCOL = "oo.adjudication.measurements/v1"

    def __init__(self, path, facts_sha256):
        self.path, self.facts, self.entries = path, facts_sha256, {}
        if path and Path(path).exists():
            if Path(path).is_symlink() or Path(path).stat().st_size > 64_000_000:
                raise ValueError("adjudication_cache_file")
            data = json.loads(Path(path).read_text())
            if data.get("protocol") != self.PROTOCOL:
                raise ValueError("adjudication_cache_protocol")
            self.entries = {k: v for k, v in data.get("entries", {}).items()}

    def key(self, signature):
        return lib.digest({"facts_sha256": self.facts, "edge": signature, "protocol": self.PROTOCOL})

    def get(self, signature):
        value = self.entries.get(self.key(signature))
        return validate_measurement(value) if value is not None else None

    def put(self, signature, value):
        self.entries[self.key(signature)] = value

    def save(self):
        if not self.path:
            return
        path = Path(self.path)
        tmp = path.with_name(path.name + ".tmp")
        with os.fdopen(os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600), "w") as f:
            f.write(lib.canonical({"protocol": self.PROTOCOL, "entries": self.entries}))
        os.replace(tmp, path)


def collect(db, command, tables, input_sha256, budget=65536, cache_path=None):
    if type(budget) is not int or budget < 0:
        raise ValueError("relationship_check_budget")
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
        rows = next(iter(profiles[dataset, n]["rows"] for n in columns), 0)
        # Declared PK, then declared UNIQUE, then an observed-unique key-like
        # text/integer column. Coincidentally unique amounts or times never qualify.
        tiers = ([(list(table["primary_key"]), "declared_pk")] if table.get("primary_key") else [])
        tiers += [(list(k), "declared_unique") for k in declared_uniques(table)]
        tiers += [([n], "observed_unique") for n in sorted(columns)
                  if key_family(columns[n]) and keylike(n) and profiles[dataset, n]["rows"] > 0
                  and profiles[dataset, n]["distinct"] == profiles[dataset, n]["rows"]
                  and [n] not in [t[0] for t in tiers]]
        key, source, valid, considered = [], "none", False, []
        measured = {"rows": rows, "non_null": 0, "distinct": 0, "null_rows": 0, "empty_rows": 0}
        for candidate, origin in tiers:
            names = [quote(columns[n]["physical_name"]) for n in candidate]
            observed = db.query(
                "select json_build_object('rows',count(*),'non_null',count(*) filter(where "
                + " and ".join(n + " is not null" for n in names)
                + "),'distinct',count(distinct ROW(" + ",".join(names) + ")),'null_rows',count(*) filter(where "
                + " or ".join(n + " is null" for n in names) + "),'empty_rows',count(*) filter(where "
                + " or ".join("btrim(" + n + "::text)=''" for n in names) + ")) from " + table["physical"])
            observed.setdefault("null_rows", observed["rows"] - observed["non_null"])
            observed.setdefault("empty_rows", 0)
            ok = (observed["rows"] > 0 and observed["rows"] == observed["non_null"] == observed["distinct"]
                  and (origin != "observed_unique" or observed["empty_rows"] == 0))
            considered.append(dict(columns=candidate, source=origin, valid=ok))
            if ok or not key:
                key, source, measured = candidate, origin, observed
            if ok:
                valid = True
                break
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
                        key=key, key_source=source, considered=considered,
                        display=display, full_scan=True)
        rule = ("entity_key." + source if valid else
                "entity_key.no_candidate" if not tiers else "entity_key.dataset")
        add("entity_key", dataset, {"table": dataset}, "adopted" if valid else "rejected",
            evidence, 1.0 if valid else 0.0, rule)
        entities.append(dict(id="entity/" + dataset, label=labels.get(dataset, dataset), table=dataset,
                             key=key if valid else [], display=display if valid else None, rows=rows))

    definitions, log_status = join_sources(db) if command["draft"]["graph"]["edges"] else ([], "unavailable")
    cache = MeasurementCache(cache_path, lib.digest(tables))
    def priority(edge):
        kind = edge_kind(edge, inventory[edge["from"]])
        model = kind == "inferred_id" and not str(edge["id"]).startswith("discovered-")
        return (KIND_ORDER["model" if model else kind], edge["id"])
    measured, unchecked, checked = [], [], 0
    try:
        for edge in sorted(command["draft"]["graph"]["edges"], key=priority):
            child, parent = inventory[edge["from"]], inventory[edge["to"]]
            columns = [next(c for c in child["columns"] if c["name"] == n) for n in edge["columns"]["from"]]
            keys = [next(c for c in parent["columns"] if c["name"] == n) for n in edge["columns"]["to"]]
            compatible = len(columns) == len(keys) and bool(columns) and all(
                type_family(c) == type_family(k) for c, k in zip(columns, keys))
            signature = [edge["from"], edge["columns"]["from"], edge["to"], edge["columns"]["to"]]
            observed = None
            if compatible:
                observed = cache.get(signature)
                if observed is None:
                    if checked >= budget:
                        # Budget is a resumable boundary, not a silent cap.
                        unchecked.append(dict(id="relation/" + edge["id"], kind=edge_kind(edge, child),
                                              **{"from": edge["from"], "to": edge["to"]},
                                              from_columns=edge["columns"]["from"],
                                              to_columns=edge["columns"]["to"]))
                        continue
                    checked += 1
                    scope = [c for c in parent["columns"] if c["name"] not in edge["columns"]["to"]
                             and (SCOPE_NAME.search(c["name"])
                                  or re.match(r"^(date|timestamp)", c["data_type"].lower()))][:4]
                    observed = validate_measurement(measure_edge(db, child, columns, parent, keys, scope))
                    cache.put(signature, observed)
            measured.append((edge, child, parent, columns, keys, compatible, observed))
    finally:
        cache.save()
    pairs = {}
    for edge, *_ in measured:
        pairs.setdefault((edge["from"], edge["to"]), set()).add(tuple(edge["columns"]["from"]))
    for edge, child, parent, columns, keys, compatible, observed in sorted(measured, key=lambda m: m[0]["id"]):
        m = observed or dict.fromkeys(MEASUREMENT_FIELDS, 0) | {"scope": {}}
        count, matched = m["non_blank_rows"], m["matched_rows"]
        ratio = matched / count if count else 0
        distinct = m["matched_distinct"] / m["child_distinct"] if m["child_distinct"] else 0
        parent_unique = (m["parent_distinct"] > 0 and m["parent_duplicate_keys"] == 0
                         and m["parent_empty_rows"] == 0)
        child_unique = m["child_distinct"] == count and count > 0
        cardinality = ("one" if child_unique else "many") + "_to_" + ("one" if parent_unique else "many")
        length_ok = all(type_family(k) != "text" or
                        profiles[child["dataset"], c["name"]]["non_null"] == 0 or
                        not re.search(r"\((\d+)\)", k["data_type"]) or
                        profiles[child["dataset"], c["name"]]["max_length"] <=
                        int(re.search(r"\((\d+)\)", k["data_type"])[1])
                        for c, k in zip(columns, keys))
        similarity = sum(c["name"] == k["name"] or bool(lib.candidate_signal(child, c, parent, k))
                         or mapping_signal(c["name"], parent, k["name"])
                         for c, k in zip(columns, keys)) / max(len(columns), 1)
        kind = edge_kind(edge, child)
        scope_columns = sorted(n for n, ok in m["scope"].items() if ok) if not parent_unique else []
        evidence = dict(containment=ratio, distinct_containment=distinct, parent_unique=parent_unique,
                        type_compatible=compatible, length_compatible=length_ok, name_similarity=similarity,
                        join_cooccurrence=join_count(definitions, child, columns, parent, keys),
                        cardinality=cardinality, rows_checked=count, child_distinct=m["child_distinct"],
                        matched_distinct=m["matched_distinct"], matched_rows=matched,
                        unmatched_rows=count - matched,
                        unmatched_distinct=m["child_distinct"] - m["matched_distinct"],
                        empty_rows=m["empty_rows"], parent_rows=m["parent_rows"],
                        parent_distinct=m["parent_distinct"], parent_empty_rows=m["parent_empty_rows"],
                        parent_duplicate_keys=m["parent_duplicate_keys"], max_fanout=m["max_fanout"],
                        normalized_collisions=m["parent_distinct"] - m["parent_normalized_distinct"],
                        scope_columns=scope_columns, kind=kind,
                        source=("declared" if kind == "declared_fk" else
                                "discovery" if str(edge["id"]).startswith("discovered-") else "model"),
                        role=(role_name(edge["columns"]["from"], edge["columns"]["to"])
                              if len(pairs[edge["from"], edge["to"]]) > 1 else None),
                        method="full_scan", query_log=log_status,
                        from_columns=edge["columns"]["from"], to_columns=edge["columns"]["to"])
        decision, rule = relation_decision(evidence, config)
        if rule == "relation.scope_required":
            evidence["kind"] = "scoped"
        identity = add("relation", edge["id"], {
            "from": edge["from"] + "." + columns[0]["name"],
            "to": edge["to"] + "." + keys[0]["name"]},
                       decision, evidence, ratio, rule)
        items[-1]["reason"] = relation_reason(rule, evidence)
        edges.append(dict(relation=identity, **{"from": "entity/" + edge["from"],
                                              "to": "entity/" + edge["to"]},
                          cardinality=cardinality, adopted=decision == "adopted"))
        if (parent["primary_key"] == edge["columns"]["to"] and
                any("code" in n for n in edge["columns"]["to"]) and
                profiles[parent["dataset"], keys[0]["name"]]["rows"] <= config["code_max_rows"]):
            code, _ = code_decision(count, matched, profiles[parent["dataset"], keys[0]["name"]]["rows"], config)
            add("code_table", edge["id"], {"from": edge["from"], "to": edge["to"]}, code,
                dict(coverage=ratio, rows_checked=count), ratio, "code_table.coverage")
    discovery = dict(status="incomplete" if unchecked else "complete", budget=budget,
                     checked=len(measured), remaining=len(unchecked),
                     unchecked=sorted(unchecked, key=lambda u: u["id"]))

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
                    items=sorted(items, key=lambda i: i["id"]), discovery=discovery,
                    graph=dict(entities=entities, edges=sorted(edges, key=lambda e: e["relation"])))
    if len({i["id"] for i in items}) != len(items):
        raise ValueError("adjudication_duplicate_id")
    document["summary_sha256"] = summary_hash(document)
    return document


def relation_quality(evidence):
    """Qualitative proof state of an adopted edge; row-count growth alone keeps it."""
    return dict(parent_unique=evidence["parent_unique"], fanout=evidence["max_fanout"] > 1,
                collision=evidence["normalized_collisions"] > 0,
                rows=coverage_class(evidence["containment"]),
                distinct=coverage_class(evidence["distinct_containment"]),
                blank=evidence["empty_rows"] > 0)


def relationship_quality_drift(db, document, tables):
    """Re-measure every adopted relationship of a confirmed adjudication on a new cut.

    Returns the relation ids whose uniqueness, fan-out, collisions or coverage
    class changed; a stored proof without the measurement counts as changed.
    """
    inventory = {t["dataset"]: t for t in tables}
    ends = {e["relation"]: (e["from"][len("entity/"):], e["to"][len("entity/"):])
            for e in document.get("graph", {}).get("edges", [])}
    drift = []
    for item in document.get("items", []):
        if item["class"] != "relation" or item["decision"] != "adopted":
            continue
        evidence = item["evidence"]
        child, parent = (inventory.get(n) for n in ends.get(item["id"], (None, None)))
        try:
            before = relation_quality(evidence)
            columns = [next(c for c in child["columns"] if c["name"] == n) for n in evidence["from_columns"]]
            keys = [next(c for c in parent["columns"] if c["name"] == n) for n in evidence["to_columns"]]
        except (KeyError, TypeError, StopIteration):
            drift.append(item["id"])
            continue
        m = validate_measurement(measure_edge(db, child, columns, parent, keys))
        count = m["non_blank_rows"]
        after = relation_quality(dict(
            parent_unique=m["parent_distinct"] > 0 and m["parent_duplicate_keys"] == 0
            and m["parent_empty_rows"] == 0, max_fanout=m["max_fanout"],
            normalized_collisions=m["parent_distinct"] - m["parent_normalized_distinct"],
            containment=m["matched_rows"] / count if count else 0,
            distinct_containment=m["matched_distinct"] / m["child_distinct"] if m["child_distinct"] else 0,
            empty_rows=m["empty_rows"]))
        if before != after:
            drift.append(item["id"])
    return sorted(drift)


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
    result = collect(db, command, facts["tables"], hashlib.sha256(raw).hexdigest(),
                     budget=config.get("max_relationship_checks", 65536),
                     cache_path=args.input.with_name("adjudication-measurements.json"))
    _, after = retained_build.facts_for(db, config)
    if before != after:
        raise ValueError("adjudication_retained_cut_changed")
    target = args.input.with_name("adjudication.json")
    if target.exists() and not target.is_symlink():
        # Only an incomplete pass may be superseded by its resumed rerun.
        if json.loads(target.read_text()).get("discovery", {}).get("status") != "incomplete":
            raise ValueError("adjudication_output_exists")
        target.unlink()
    retained_build.save(target, result)


if __name__ == "__main__":
    main()
