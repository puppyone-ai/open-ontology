"""Reviewed effective-dated dictionary definitions; replica-only measurements."""
import re

import library_selfbuild as lib
from library_observation import literal, quote


COUNTS = ("rows_checked", "matched_rows", "ambiguous_rows", "invalid_intervals",
          "overlap_pairs", "gap_pairs")


def measure(db, inventory, spec):
    if (set(spec) != {"id", "concept", "from", "dictionary", "interval", "provenance"}
            or spec["interval"] not in ("closed_open_null_end_unbounded",
                                        "closed_closed_date_null_end_unbounded")
            or not re.fullmatch(r"[a-z][a-z0-9_-]{0,62}", spec["id"])
            or not isinstance(spec["concept"], str) or not spec["concept"].strip()
            or len(spec["concept"]) > 256):
        raise ValueError("dictionary_binding_schema")
    provenance = spec["provenance"]
    if (set(provenance) != {"origin", "source_sha256", "reviewer", "reviewed_at"}
            or provenance["origin"] != "operator_reviewed"
            or not re.fullmatch(r"[0-9a-f]{64}", provenance["source_sha256"])
            or not provenance["reviewer"].strip()
            or provenance["reviewer"].lower() == "model"):
        raise ValueError("dictionary_binding_provenance")
    from datetime import date
    date.fromisoformat(provenance["reviewed_at"])
    fact, dictionary = spec["from"], spec["dictionary"]
    if ((fact is not None and set(fact) != {"dataset", "code", "at"})
            or set(dictionary) != {"dataset", "code", "label", "discriminator",
                                   "value", "valid_from", "valid_to"}
            or not isinstance(dictionary["value"], str)
            or not 1 <= len(dictionary["value"]) <= 256):
        raise ValueError("dictionary_binding_columns")

    def columns(end, fields):
        table = inventory[end["dataset"]]
        names = {c["name"]: c["physical_name"] for c in table["columns"]}
        return table["physical"], {k: quote(names[end[k]]) for k in fields}

    dtable, d = columns(dictionary, ("code", "label", "discriminator", "valid_from", "valid_to"))
    inclusive = spec["interval"] == "closed_closed_date_null_end_unbounded"
    if inclusive:
        types = {c["name"]: c.get("data_type") for c in inventory[dictionary["dataset"]]["columns"]}
        if any(types[dictionary[k]] != "date" for k in ("valid_from", "valid_to")):
            raise ValueError("dictionary_binding_date_type")
    end_op, invalid_op = ("<=", "<") if inclusive else ("<", "<=")
    gap = "(hi + interval '1 day') < next_lo" if inclusive else "hi < next_lo"
    matches = "select 0 as n where false"
    if fact is not None:
        ftable, f = columns(fact, ("code", "at"))
        matches = f"""select (select count(*) from d where d.code::text=c.{f['code']}::text
          and c.{f['at']} >= d.lo and (d.hi is null or c.{f['at']} {end_op} d.hi)) as n
          from {ftable} c where c.{f['code']} is not null"""
    # A stable row number distinguishes duplicate intervals, even without a PK.
    sql = f"""
with d as (
 select row_number() over () as rid, {d['code']} as code,
 {d['label']} as label, {d['valid_from']} as lo, {d['valid_to']} as hi
 from {dtable} where {d['discriminator']}::text={literal(dictionary['value'])}
), matches as (
 {matches}
), intervals as (
 select *, lead(lo) over(partition by code order by lo, rid) as next_lo from d
)
select json_build_object(
 'rows_checked',(select count(*) from matches),
 'matched_rows',(select count(*) from matches where n=1),
 'ambiguous_rows',(select count(*) from matches where n>1),
 'invalid_intervals',(select count(*) from d where code is null or label is null
     or lo is null or hi{invalid_op}lo),
 'overlap_pairs',(select count(*) from d a join d b on a.rid<b.rid and a.code=b.code
     and (a.hi is null or b.lo{end_op}a.hi) and (b.hi is null or a.lo{end_op}b.hi)),
 'gap_pairs',(select count(*) from intervals where {gap}))
"""
    # Keep the existing replica adapter's SELECT-only admission unchanged.
    observed = db.query("select (" + sql + ")")
    if (set(observed) != set(COUNTS)
            or any(type(observed[k]) is not int or observed[k] < 0 for k in COUNTS)
            or observed["matched_rows"] + observed["ambiguous_rows"] > observed["rows_checked"]):
        raise ValueError("dictionary_binding_measurement")
    complete = (observed["rows_checked"] > 0
                and observed["matched_rows"] == observed["rows_checked"]
                and all(observed[k] == 0 for k in COUNTS[2:]))
    return ("complete" if complete else "incomplete",
            dict(observed, binding_sha256=lib.digest(spec)))
