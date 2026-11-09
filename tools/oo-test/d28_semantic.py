"""Synthetic semantic profiles; gold is independent of the detector output."""
import adjudication
from library_observation import literal, quote


def semantic_probes(admin, db):
    cases = [
        ("cjk_names", ["张伟", "李芳", "欧阳娜娜", "王小明"], True),
        ("latin_names", ["Alice Smith", "John Johnson", "Maria Garcia", "Emma Wilson"], True),
        ("cjk_addresses", ["北京市朝阳区建国路88号", "上海市南京路12号"], True),
        ("latin_addresses", ["123 Main Street", "42 Oak Avenue", "8 Elm Road"], True),
        ("contact_notes", ["Contact: alice@example.invalid tomorrow",
                           "电话：13800138000，下午联系"], True),
        ("date_of_birth", ["1987-03-14", "2001-11-09"], True),
        ("dob", ["1990/01/02", "1985/12/13"], True),
        ("email_label", ["Contact label", "Priority customer", "Standard delivery"], False),
        ("company", ["王氏公司", "上海物流", "Acme Corporation", "North Warehouse"], False),
        ("status", ["New", "Closed", "In transit", "Pending"], False),
        ("region", ["北京市", "上海市", "广东省", "North Region"], False),
        ("ordered_at", ["2026-01-01", "2026-02-01"], False),
        ("invalid_dob", ["1999-02-30", "2000-13-40", "pending"], False),
        ("product", ["Steel Beam", "Blue Widget", "设备型号A"], False),
    ]
    counts = dict(tp=0, fp=0, fn=0, tn=0)
    profiles = {}
    for index, (name, values, positive) in enumerate(cases):
        table = '"t6"."semantic_' + str(index) + '"'
        admin("CREATE TABLE " + table + "(" + quote(name) + " text); INSERT INTO " + table +
              " VALUES " + ",".join("(" + literal(v) + ")" for v in values) +
              "; GRANT SELECT ON " + table + " TO d28_reader;")
        profile = adjudication.column_profile(db, {"physical": table},
                                             {"name": name, "physical_name": name})
        decision = adjudication.sensitive_decision(profile, True, adjudication.thresholds())[0]
        predicted = decision == "hidden"
        counts[("t" if predicted == positive else "f") + ("p" if predicted else "n")] += 1
        profiles[name] = dict(profile=profile, decision=decision, positive=positive)
    counts["precision"] = counts["tp"] / max(1, counts["tp"] + counts["fp"])
    counts["recall"] = counts["tp"] / max(1, counts["tp"] + counts["fn"])
    result = dict(counts=counts, profiles=profiles)
    if counts["precision"] < .9 or counts["recall"] < .9:
        raise AssertionError(result)
    return result
