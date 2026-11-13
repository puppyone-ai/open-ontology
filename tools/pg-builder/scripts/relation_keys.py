"""Key vocabulary shared by adjudication and the data-only boundary.

A candidate key is a declared PRIMARY KEY or UNIQUE constraint, or an
observed-unique column whose type is text or integral and whose name is
key-like. Amounts, floats, times and booleans are never keys, however unique.
"""
import copy
import re


KEYLIKE = re.compile(r"((^|_)(id|uuid|guid|code|cd|no|num|nbr|key|dm|bh|ddh)$|ydh$|"
                     r"[a-z0-9](Id|ID|Code|No|Key)$|编号|编码|代码|单号)")
NOT_KEY = re.compile(r"(paid|valid|void|said|grid|fluid|liquid|hybrid|android|rapid|solid|"
                     r"avid|vivid|humid|lucid|acid)$", re.I)
def keylike(name):
    return bool(KEYLIKE.search(name) or re.search(r"[a-z]id$", name, re.I) and not NOT_KEY.search(name))


def key_family(column):
    """Entity keys are text or integral; never scaled numbers, floats, times or booleans."""
    kind = column["data_type"].lower()
    if re.match(r"^(smallint|integer|bigint|int\b|int[248]\b|serial|bigserial)", kind):
        return "integer"
    if re.match(r"^(numeric|decimal)\s*\(\s*\d+\s*(,\s*0\s*)?\)$", kind):
        return "integer"
    if re.match(r"^(numeric|decimal)$", kind):
        profile = column.get("profile") or {}
        # Unconstrained NUMBER is integral only if every observed value is digits.
        if profile.get("non_null") and profile.get("digits_rows") == profile["non_null"]:
            return "integer"
        return None
    if re.match(r"^(text|character|varchar|char\b|uuid)", kind):
        return "text"
    return None


def declared_uniques(table):
    """UNIQUE constraints from captured DDL text; names map to logical columns."""
    names = {c.get("source_name", c["name"]).lower(): c["name"] for c in table["columns"]}
    names.update({c["name"].lower(): c["name"] for c in table["columns"]})
    found = [list(k) for k in table.get("unique_keys", [])]
    for sql in table.get("constraints_sql") or []:
        match = re.search(r"\bUNIQUE\b[^(]*\(([^)]*)\)", sql, re.I)
        if match:
            columns = [names.get(re.sub(r"[`\"\[\]\s]|\(\d+\)", "", c).lower())
                       for c in match[1].split(",")]
            if columns and None not in columns:
                found.append(columns)
    return sorted({tuple(k) for k in found if k and k != table.get("primary_key")})


def observed_unique(column):
    profile = column.get("profile") or {}
    return bool(profile.get("non_null")) and profile.get("distinct") == profile["non_null"] \
        and not profile.get("blank_rows")



def key_columns(table):
    """Columns whose quality decides whether stored relationship proofs still hold."""
    result = set(table.get("primary_key") or [])
    for key in declared_uniques(table):
        result.update(key)
    for column in table["columns"]:
        if key_family(column) and keylike(column["name"]) and observed_unique(column):
            result.add(column["name"])
    return result


def quality(profile):
    """Qualitative state only: plain row-count growth must not change it."""
    non_null, distinct = profile.get("non_null", 0), profile.get("distinct", 0)
    return {"empty": not non_null, "unique": bool(non_null) and distinct == non_null,
            "duplicates": non_null > distinct, "null_present": bool(profile.get("null_count"))
            or profile.get("null_present", False), "blank_present": bool(profile.get("blank_rows"))}


def key_quality(facts):
    """Key and relationship-end quality per table, for the data-only boundary."""
    ends = {}
    for relation in facts.get("relationship_candidates", []):
        for end in ("from", "to"):
            ends.setdefault(relation[end]["dataset"], set()).update(relation[end]["columns"])
    result = {}
    for table in facts["tables"]:
        names = key_columns(table) | ends.get(table["dataset"], set())
        columns = {c["name"]: c for c in table["columns"]}
        result[table["dataset"]] = {
            name: quality(copy.deepcopy(columns[name].get("profile", {})))
            for name in sorted(names) if name in columns}
    return result


def coverage_class(ratio):
    if ratio is None:
        return "unmeasured"
    ratio = float(ratio)
    return ("none" if ratio == 0 else "complete" if ratio == 1 else "strict" if ratio >= .98
            else "signal" if ratio >= .9 else "partial")
