"""Audit-record shape for supply-chain/policy.md.

The checks below read a JSON record. They do not run cargo audit, cargo deny,
or an image or native scanner, and they do not admit a package into
dependency-baseline.json.

Publication and adoption values on an audited record are full RFC3339 UTC
timestamps. ``age_days_at_adoption`` is the integer count of completed 24-hour
intervals. The seven-day decision compares the exact elapsed duration with
``7 * 86400`` seconds. Float ages stored on the dependency baseline are not
this record's age, and this module does not rewrite that baseline.
"""

from __future__ import annotations

import re
from datetime import date, datetime, timedelta, timezone
from decimal import Decimal

from qa_license import evaluate_license

_RFC3339_UTC = re.compile(r"^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(\.\d+)?(?:[Zz]|\+00:00)$")
_DIGEST = re.compile(r"(?:sha256:[0-9a-f]{64}|git:[0-9a-f]{40})")
_DAY_SECONDS = Decimal(86400)
_SEVEN_SECONDS = Decimal(7 * 86400)
_CLEAN_DISPOSITIONS = frozenset({"no-known-advisories", "resolved-before-adoption"})
_EMPTY_WORDS = frozenset({"", "pending", "absent", "none", "n/a", "na", "true", "unknown", "not-scanned"})

APPOINTED_ROLES = frozenset(
    {
        "data-owner",
        "domain-steward",
        "eval-owner",
        "ops-owner",
        "pipeline-owner",
        "release-owner",
        "rust-owner",
        "security-owner",
        "supply-chain-owner",
    }
)


def template_errors(doc: dict) -> list[str]:
    errors: list[str] = []
    if doc.get("schema_version") != 1:
        errors.append("schema_version")
    if doc.get("template") is not True:
        errors.append("template flag")
    if doc.get("audited") is not False:
        errors.append("audited must be false on the blank template")
    if doc.get("record_status") != "blank-template":
        errors.append("record_status")
    if doc.get("do_not_treat_as_baseline") is not True:
        errors.append("do_not_treat_as_baseline")
    if doc.get("exception") is not None:
        errors.append("exception must be null")
    if doc.get("baseline_file") != "supply-chain/dependency-baseline.json":
        errors.append("baseline_file")
    if doc.get("reviewers_assigned") != []:
        errors.append("reviewers_assigned")
    package = doc.get("package")
    if not isinstance(package, dict):
        return errors + ["package"]
    for key in (
        "name",
        "version",
        "license_spdx",
        "official_source",
        "digest",
        "publication_date",
        "adoption_date",
        "age_days_at_adoption",
        "seven_day_rule",
        "msrv",
        "default_features_disabled",
        "dual_license_choice",
        "prohibited_license",
        "ecosystem",
        "repository",
    ):
        if package.get(key) is not None:
            errors.append(f"blank package.{key}")
    for key in ("manifests", "features_enabled", "license_evidence", "license_obligations", "native_transitive"):
        if package.get(key) != []:
            errors.append(f"blank package.{key}")
    if package.get("weak_copyleft_review") is not None:
        errors.append("blank package.weak_copyleft_review")
    advisories = doc.get("advisories")
    if not isinstance(advisories, dict):
        errors.append("advisories")
    else:
        for key in ("scan_recorded", "scanner", "evidence", "disposition", "known_exploitable_high_severity"):
            if advisories.get(key) is not None:
                errors.append(f"blank advisories.{key}")
    return errors


class _Stamp:
    def __init__(self, epoch: Decimal) -> None:
        self.epoch = epoch


def _epoch_from_datetime(value: datetime) -> Decimal:
    utc = value.astimezone(timezone.utc)
    days = (utc.date() - date(1970, 1, 1)).days
    return (
        Decimal(days) * _DAY_SECONDS
        + Decimal(utc.hour * 3600 + utc.minute * 60 + utc.second)
        + (Decimal(utc.microsecond) / Decimal(1000000))
    )


def _require_as_of(as_of: object) -> tuple[Decimal | None, str | None]:
    if isinstance(as_of, datetime) and as_of.tzinfo is not None and as_of.utcoffset() == timedelta(0):
        return _epoch_from_datetime(as_of), None
    return None, "as_of must be an aware UTC datetime"


def _parse_stamp(value: object, field: str, as_of_epoch: Decimal) -> tuple[_Stamp | None, str | None]:
    if not isinstance(value, str):
        return None, f"invalid {field}"
    match = _RFC3339_UTC.fullmatch(value)
    if match is None:
        return None, f"invalid {field}"
    year, month, day, hour, minute, second = (int(match.group(index)) for index in range(1, 7))
    try:
        date(year, month, day)
    except ValueError:
        return None, f"invalid {field}"
    if hour > 23 or minute > 59 or second > 59:
        return None, f"invalid {field}"
    fraction = match.group(7)
    fractional = Decimal(0)
    if fraction:
        digits = fraction[1:]
        fractional = Decimal(digits) / (Decimal(10) ** len(digits))
    days = (date(year, month, day) - date(1970, 1, 1)).days
    epoch = Decimal(days) * _DAY_SECONDS + Decimal(hour * 3600 + minute * 60 + second) + fractional
    now_epoch = _epoch_from_datetime(datetime.now(timezone.utc))
    limit = as_of_epoch if as_of_epoch <= now_epoch else now_epoch
    if epoch > limit:
        return None, f"future {field}"
    return _Stamp(epoch), None


def _completed_days(elapsed: Decimal) -> int:
    return int(elapsed // _DAY_SECONDS)


def _advisory_errors(block: object) -> list[str]:
    if not isinstance(block, dict):
        return ["advisory scan not recorded"]
    errors: list[str] = []
    if block.get("scan_recorded") is not True:
        errors.append("advisory scan not recorded")
    scanner = block.get("scanner")
    if not isinstance(scanner, str) or scanner.strip().lower() in _EMPTY_WORDS:
        errors.append("advisory scanner")
    evidence = block.get("evidence")
    if not isinstance(evidence, str) or "/" not in evidence or evidence.strip().lower() in _EMPTY_WORDS:
        errors.append("advisory evidence")
    if block.get("disposition") not in _CLEAN_DISPOSITIONS:
        errors.append("advisory disposition")
    high = block.get("known_exploitable_high_severity")
    if high is True:
        errors.append("known exploitable high-severity")
    elif high is not False:
        errors.append("known_exploitable_high_severity")
    return errors


def _regression_errors(evidence: object) -> list[str]:
    if not isinstance(evidence, dict):
        return ["exception.regression_evidence"]
    errors: list[str] = []
    command = evidence.get("command")
    if not isinstance(command, str) or len(command.strip()) < 3 or command.strip().lower() in _EMPTY_WORDS:
        errors.append("exception.regression_evidence.command")
    exit_code = evidence.get("exit_code")
    if type(exit_code) is not int:
        errors.append("exception.regression_evidence.exit_code")
    artifact = evidence.get("artifact")
    if not isinstance(artifact, str) or "/" not in artifact or artifact.strip().lower() in _EMPTY_WORDS:
        errors.append("exception.regression_evidence.artifact")
    return errors


def _exception_errors(doc: dict, elapsed: Decimal | None, appointed_names: list[str]) -> list[str]:
    exception = doc.get("exception")
    errors: list[str] = []
    if not isinstance(exception, dict):
        return ["exception kind"]
    if exception.get("kind") != "security-fix-seven-day":
        errors.append("exception kind")
    if not appointed_names:
        errors.append("no appointed owner")
    owner_name = exception.get("owner_name")
    if not isinstance(owner_name, str) or owner_name not in appointed_names:
        errors.append("exception.owner_name")
    if exception.get("owner_role") not in APPOINTED_ROLES:
        errors.append("exception.owner_role")
    reason = exception.get("reason")
    if not isinstance(reason, str) or not reason.strip():
        errors.append("exception.reason")
    errors.extend(_regression_errors(exception.get("regression_evidence")))
    if exception.get("waive") != []:
        errors.append("exception.waive")
    if elapsed is not None and elapsed >= _SEVEN_SECONDS:
        errors.append("exception used when the seven-day rule already passes")
    return errors


def _native_errors(entries: object, *, as_of: datetime) -> list[str]:
    del as_of
    if not isinstance(entries, list):
        return ["native_transitive"]
    errors: list[str] = []
    for index, entry in enumerate(entries):
        label = f"native_transitive[{index}]"
        if isinstance(entry, str):
            errors.append(f"{label} is a string, not a license object: {entry}")
            continue
        if not isinstance(entry, dict):
            errors.append(f"{label} must be an object")
            continue
        for key in ("name", "version"):
            if not isinstance(entry.get(key), str) or not entry[key].strip():
                errors.append(f"{label}: missing {key}")
        if "dual_license_choice" not in entry:
            errors.append(f"{label}: dual_license_choice")
        errors.extend(
            evaluate_license(
                label=label,
                expression=entry.get("license_spdx"),
                choice=entry.get("dual_license_choice"),
                evidence=entry.get("license_evidence"),
                obligations=entry.get("license_obligations"),
                review=entry.get("weak_copyleft_review"),
            )
        )
    return errors


def audited_record_errors(
    doc: dict,
    *,
    as_of: datetime,
    appointed_names: list[str] | None = None,
) -> list[str]:
    """Return admission errors. appointed_names is who the repository has named.

    ``as_of`` is an aware UTC datetime. A date, or a naive time, is not read as
    midnight and cannot make a later instant on that calendar day admissible.
    An empty appointment list is the state of this tree while CODEOWNERS says
    OWNER_PENDING. A caller-supplied fixture list is only for the in-memory
    shape control.
    """

    appointed = list(appointed_names or [])
    errors: list[str] = []
    as_of_epoch, as_of_error = _require_as_of(as_of)
    if as_of_error or as_of_epoch is None:
        return [as_of_error or "as_of must be an aware UTC datetime"]
    if doc.get("audited") is not True or doc.get("template") is not False:
        errors.append("audited record flags")
    package = doc.get("package")
    if not isinstance(package, dict):
        return errors + ["package"]
    for key in ("ecosystem", "name", "version", "official_source", "repository", "msrv"):
        if not isinstance(package.get(key), str) or not package[key].strip():
            errors.append(f"missing {key}")
    if not isinstance(package.get("manifests"), list) or not package["manifests"]:
        errors.append("manifests")
    if not isinstance(package.get("features_enabled"), list):
        errors.append("features_enabled")
    digest = package.get("digest")
    if not isinstance(digest, str) or _DIGEST.fullmatch(digest) is None:
        errors.append("digest")
    if type(package.get("default_features_disabled")) is not bool:
        errors.append("default_features_disabled")
    if "dual_license_choice" not in package:
        errors.append("package: dual_license_choice")
    if package.get("prohibited_license") is not False:
        errors.append("prohibited_license flag")
    published, published_error = _parse_stamp(package.get("publication_date"), "publication_date", as_of_epoch)
    adopted, adopted_error = _parse_stamp(package.get("adoption_date"), "adoption_date", as_of_epoch)
    if published_error:
        errors.append(published_error)
    if adopted_error:
        errors.append(adopted_error)
    elapsed: Decimal | None = None
    if published and adopted:
        if adopted.epoch < published.epoch:
            errors.append("adoption_date precedes publication_date")
        else:
            elapsed = adopted.epoch - published.epoch
    age = package.get("age_days_at_adoption")
    if isinstance(age, bool) or type(age) is not int:
        errors.append("age_days_at_adoption")
    elif elapsed is not None and age != _completed_days(elapsed):
        errors.append("age_days_at_adoption does not match the dates")
    rule = package.get("seven_day_rule")
    if rule == "pass":
        if elapsed is not None and elapsed < _SEVEN_SECONDS:
            errors.append("age under 7")
        if doc.get("exception") is not None:
            errors.append("exception present on a passing age rule")
    elif rule == "security-exception":
        errors.extend(_exception_errors(doc, elapsed, appointed))
    else:
        errors.append("seven_day_rule")
    errors.extend(
        evaluate_license(
            label="package",
            expression=package.get("license_spdx"),
            choice=package.get("dual_license_choice"),
            evidence=package.get("license_evidence"),
            obligations=package.get("license_obligations"),
            review=package.get("weak_copyleft_review"),
        )
    )
    errors.extend(_native_errors(package.get("native_transitive"), as_of=as_of))
    errors.extend(_advisory_errors(doc.get("advisories")))
    return errors


def license_evidence(*spdx_ids: str) -> list[dict]:
    return [{"spdx": item, "source": f"LICENSE-{item}"} for item in spdx_ids]


def clean_advisories() -> dict:
    return {
        "scan_recorded": True,
        "scanner": "recorded-advisory-scan",
        "evidence": "evidence/synthetic/advisory.txt",
        "disposition": "no-known-advisories",
        "known_exploitable_high_severity": False,
    }


def synthetic_audited(**overrides: object) -> dict:
    package = {
        "ecosystem": "cargo",
        "name": "example-synthetic-crate",
        "version": "1.2.3",
        "official_source": "https://example.invalid/example-synthetic-crate",
        "repository": "https://example.invalid/example-synthetic-crate.git",
        "manifests": ["crates/example-synthetic/Cargo.toml"],
        "publication_date": "2026-01-01T00:00:00Z",
        "adoption_date": "2026-02-01T00:00:00Z",
        "age_days_at_adoption": 31,
        "seven_day_rule": "pass",
        "msrv": "1.94.0",
        "features_enabled": [],
        "default_features_disabled": True,
        "license_spdx": "MIT",
        "license_evidence": license_evidence("MIT"),
        "license_obligations": [],
        "dual_license_choice": None,
        "weak_copyleft_review": None,
        "native_transitive": [],
        "digest": "sha256:" + ("ab" * 32),
        "prohibited_license": False,
    }
    document = {
        "template": False,
        "audited": True,
        "package": package,
        "exception": None,
        "advisories": clean_advisories(),
    }
    package_overrides = overrides.pop("package", {})
    if isinstance(package_overrides, dict):
        package.update(package_overrides)
    document.update(overrides)
    return document


def _expect(errors: list[str], needle: str) -> str | None:
    if any(needle in item for item in errors):
        return None
    return f"missing {needle!r} in {errors}"


def self_test(as_of: datetime) -> list[tuple[str, str | None]]:
    """Negative controls from the review, plus the safe records that must pass."""

    results: list[tuple[str, str | None]] = []

    def reject(name: str, doc: dict, needle: str, appointed: list[str] | None = None) -> None:
        errors = audited_record_errors(doc, as_of=as_of, appointed_names=appointed)
        results.append((name, _expect(errors, needle)))

    def accept(name: str, doc: dict, appointed: list[str] | None = None) -> None:
        errors = audited_record_errors(doc, as_of=as_of, appointed_names=appointed)
        results.append((name, None if not errors else f"unexpected {errors}"))

    accept("control:mit", synthetic_audited())
    accept(
        "control:mit-or-apache",
        synthetic_audited(
            package={
                "license_spdx": "MIT OR Apache-2.0",
                "dual_license_choice": "Apache-2.0",
                "license_evidence": license_evidence("MIT", "Apache-2.0"),
            }
        ),
    )
    accept(
        "control:boost-not-busl",
        synthetic_audited(
            package={
                "license_spdx": "BSL-1.0",
                "license_evidence": license_evidence("BSL-1.0"),
            }
        ),
    )
    accept(
        "control:mit-and-unicode",
        synthetic_audited(
            package={
                "license_spdx": "MIT AND Unicode-3.0",
                "license_evidence": license_evidence("MIT", "Unicode-3.0"),
                "license_obligations": [
                    {"spdx": "MIT", "obligations": ["Retain the copyright and permission notice."]},
                    {
                        "spdx": "Unicode-3.0",
                        "obligations": ["Ship the Unicode copyright and permission notice with the data."],
                    },
                ],
            }
        ),
    )
    accept(
        "control:parenthesized-or-and-unicode",
        synthetic_audited(
            package={
                "license_spdx": "(MIT OR Apache-2.0) AND Unicode-3.0",
                "dual_license_choice": "Apache-2.0",
                "license_evidence": license_evidence("MIT", "Apache-2.0", "Unicode-3.0"),
                "license_obligations": [
                    {"spdx": "Apache-2.0", "obligations": ["Retain NOTICE."]},
                    {
                        "spdx": "Unicode-3.0",
                        "obligations": ["Ship the Unicode copyright and permission notice with the data."],
                    },
                ],
            }
        ),
    )
    accept(
        "control:weak-copyleft-review",
        synthetic_audited(
            package={
                "license_spdx": "LGPL-2.1-only",
                "license_evidence": license_evidence("LGPL-2.1-only"),
                "weak_copyleft_review": {
                    "reviewer_name": "Example Reviewer",
                    "reviewer_role": "supply-chain-owner",
                    "obligations": ["Offer the LGPL source and a relink path for this library."],
                    "evidence": "evidence/synthetic/lgpl-review.md",
                    "scope": ["LGPL-2.1-only"],
                },
            }
        ),
    )
    native_ok = {
        "name": "example-native",
        "version": "1.0.0",
        "license_spdx": "MIT",
        "license_evidence": license_evidence("MIT"),
        "dual_license_choice": None,
    }
    accept("control:native-object", synthetic_audited(package={"native_transitive": [native_ok]}))
    accept(
        "control:default-features-false-is-explicit",
        synthetic_audited(package={"default_features_disabled": False}),
    )

    reject("negative:gpl-3.0-only", synthetic_audited(package={"license_spdx": "GPL-3.0-only", "prohibited_license": False}), "strong copyleft")
    reject("negative:lgpl-without-review", synthetic_audited(package={"license_spdx": "LGPL-2.1-only", "license_evidence": license_evidence("LGPL-2.1-only")}), "weak-copyleft review")
    reject("negative:licenseref", synthetic_audited(package={"license_spdx": "LicenseRef-Proprietary"}), "license not admissible")
    reject("negative:mit-or-agpl", synthetic_audited(package={"license_spdx": "MIT OR AGPL-3.0-only", "dual_license_choice": "MIT", "prohibited_license": False, "license_evidence": license_evidence("MIT", "AGPL-3.0-only")}), "prohibited license")
    reject("negative:elastic-or", synthetic_audited(package={"license_spdx": "MIT OR Elastic-2.0", "dual_license_choice": "MIT", "prohibited_license": False}), "prohibited license")
    reject("negative:busl", synthetic_audited(package={"license_spdx": "BUSL-1.1", "prohibited_license": False}), "prohibited license")
    reject("negative:cc0", synthetic_audited(package={"license_spdx": "CC0-1.0"}), "license not admissible")
    reject("negative:with-expression", synthetic_audited(package={"license_spdx": "Apache-2.0 WITH LLVM-exception"}), "unsupported license expression")
    reject("negative:or-later-plus", synthetic_audited(package={"license_spdx": "MIT+"}), "unsupported license expression")
    reject("negative:choice-missing", synthetic_audited(package={"license_spdx": "MIT OR Apache-2.0", "license_evidence": license_evidence("MIT", "Apache-2.0")}), "dual_license_choice required")
    reject("negative:choice-not-offered", synthetic_audited(package={"license_spdx": "MIT OR Apache-2.0", "dual_license_choice": "BSD-3-Clause", "license_evidence": license_evidence("MIT", "Apache-2.0", "BSD-3-Clause")}), "not an alternative")
    reject(
        "negative:and-obligations-one-leaf",
        synthetic_audited(
            package={
                "license_spdx": "MIT AND Unicode-3.0",
                "license_evidence": license_evidence("MIT", "Unicode-3.0"),
                "license_obligations": [
                    {"spdx": "MIT", "obligations": ["Retain the copyright and permission notice."]},
                ],
            }
        ),
        "obligations do not cover Unicode-3.0",
    )
    reject(
        "negative:weak-review-bool",
        synthetic_audited(
            package={
                "license_spdx": "MPL-2.0",
                "license_evidence": license_evidence("MPL-2.0"),
                "weak_copyleft_review": True,
            }
        ),
        "weak-copyleft review must name",
    )
    reject(
        "negative:weak-review-wrong-scope",
        synthetic_audited(
            package={
                "license_spdx": "LGPL-2.1-only AND MPL-2.0",
                "license_evidence": license_evidence("LGPL-2.1-only", "MPL-2.0"),
                "license_obligations": [
                    {"spdx": "LGPL-2.1-only", "obligations": ["Offer source for the LGPL library."]},
                    {"spdx": "MPL-2.0", "obligations": ["Keep the MPL notice on this file."]},
                ],
                "weak_copyleft_review": {
                    "reviewer_name": "Example Reviewer",
                    "reviewer_role": "supply-chain-owner",
                    "obligations": ["LGPL source offer only."],
                    "evidence": "evidence/synthetic/lgpl-review.md",
                    "scope": ["LGPL-2.1-only"],
                },
            }
        ),
        "does not cover MPL-2.0",
    )
    reject(
        "negative:native-string-sspl",
        synthetic_audited(package={"native_transitive": ["SSPL-1.0"]}),
        "is a string, not a license object: SSPL-1.0",
    )
    reject(
        "negative:native-object-sspl",
        synthetic_audited(
            package={
                "native_transitive": [
                    {
                        "name": "bad-native",
                        "version": "1",
                        "license_spdx": "SSPL-1.0",
                        "license_evidence": license_evidence("SSPL-1.0"),
                        "dual_license_choice": None,
                    }
                ]
            }
        ),
        "prohibited license: SSPL-1.0",
    )
    reject(
        "negative:native-weak-not-covered-by-parent",
        synthetic_audited(
            package={
                "native_transitive": [
                    {
                        "name": "child-lgpl",
                        "version": "1",
                        "license_spdx": "LGPL-2.1-only",
                        "license_evidence": license_evidence("LGPL-2.1-only"),
                        "dual_license_choice": None,
                    }
                ]
            }
        ),
        "native_transitive[0]: weak-copyleft review",
    )
    reject(
        "negative:age-mismatch",
        synthetic_audited(
            package={
                "publication_date": "2026-09-30T00:00:00Z",
                "adoption_date": "2026-10-01T00:00:00Z",
                "age_days_at_adoption": 31,
                "seven_day_rule": "pass",
            }
        ),
        "does not match the dates",
    )
    reject(
        "negative:age-under-7",
        synthetic_audited(
            package={
                "publication_date": "2026-09-30T00:00:00Z",
                "adoption_date": "2026-10-01T00:00:00Z",
                "age_days_at_adoption": 1,
                "seven_day_rule": "pass",
            }
        ),
        "age under 7",
    )
    later = (as_of + timedelta(days=40)).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    later_still = (as_of + timedelta(days=54)).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    reject(
        "negative:future-date",
        synthetic_audited(package={"publication_date": later, "adoption_date": later_still, "age_days_at_adoption": 14}),
        "future publication_date",
    )
    reject("negative:invalid-date", synthetic_audited(package={"publication_date": "2026-02-31T00:00:00Z"}), "invalid publication_date")
    reject("negative:non-leap-day", synthetic_audited(package={"publication_date": "2026-02-29T00:00:00Z"}), "invalid publication_date")
    omit = synthetic_audited()
    del omit["package"]["default_features_disabled"]
    reject("negative:omitted-default-features", omit, "default_features_disabled")
    reject("negative:string-default-features", synthetic_audited(package={"default_features_disabled": "true"}), "default_features_disabled")
    no_advisory = synthetic_audited()
    del no_advisory["advisories"]
    reject("negative:advisory-absent", no_advisory, "advisory scan not recorded")
    pending = synthetic_audited()
    pending["advisories"] = {
        "scan_recorded": False,
        "scanner": "pending",
        "evidence": "pending",
        "disposition": "pending",
        "known_exploitable_high_severity": False,
    }
    reject("negative:advisory-pending", pending, "advisory disposition")
    high = synthetic_audited()
    high["advisories"] = {
        "scan_recorded": True,
        "scanner": "recorded-advisory-scan",
        "evidence": "evidence/synthetic/advisory.txt",
        "disposition": "waived",
        "known_exploitable_high_severity": True,
    }
    high_errors = audited_record_errors(high, as_of=as_of, appointed_names=[])
    high_ok = _expect(high_errors, "known exploitable high-severity") or _expect(high_errors, "advisory disposition")
    results.append(("negative:advisory-high-not-waivable", high_ok))

    role_only = synthetic_audited(
        package={
            "publication_date": "2026-09-30T00:00:00Z",
            "adoption_date": "2026-10-01T00:00:00Z",
            "age_days_at_adoption": 1,
            "seven_day_rule": "security-exception",
        },
        exception={
            "kind": "security-fix-seven-day",
            "owner_role": "security-owner",
            "reason": "synthetic negative",
            "regression_evidence": "evidence/none",
            "waive": [],
        },
    )
    reject("negative:young-exception-role-only", role_only, "no appointed owner")
    complete_exception = {
        "kind": "security-fix-seven-day",
        "owner_name": "Example Person",
        "owner_role": "security-owner",
        "reason": "The previous version is the one named in the advisory.",
        "regression_evidence": {
            "command": "cargo test -p example-synthetic -- --exact age_case",
            "exit_code": 0,
            "artifact": "evidence/synthetic/age-regression.log",
        },
        "waive": [],
    }
    young = synthetic_audited(
        package={
            "publication_date": "2026-09-30T00:00:00Z",
            "adoption_date": "2026-10-01T00:00:00Z",
            "age_days_at_adoption": 1,
            "seven_day_rule": "security-exception",
        },
        exception=complete_exception,
    )
    reject("negative:exception-while-nobody-appointed", young, "no appointed owner")
    accept(
        "control:exception-shape-if-owner-appointed-in-memory",
        young,
        appointed=["Example Person"],
    )
    bool_exit = synthetic_audited(
        package={
            "publication_date": "2026-09-30T00:00:00Z",
            "adoption_date": "2026-10-01T00:00:00Z",
            "age_days_at_adoption": 1,
            "seven_day_rule": "security-exception",
        },
        exception={
            "kind": "security-fix-seven-day",
            "owner_name": "Example Person",
            "owner_role": "security-owner",
            "reason": "The previous version is the one named in the advisory.",
            "regression_evidence": {
                "command": "cargo test -p example-synthetic -- --exact age_case",
                "exit_code": True,
                "artifact": "evidence/synthetic/age-regression.log",
            },
            "waive": [],
        },
    )
    reject("negative:exception-bool-exit-code", bool_exit, "exit_code", appointed=["Example Person"])

    def aged(publication: str, adoption: str, age: object, rule: str = "pass") -> dict:
        return synthetic_audited(
            package={
                "publication_date": publication,
                "adoption_date": adoption,
                "age_days_at_adoption": age,
                "seven_day_rule": rule,
            }
        )

    reject(
        "negative:date-only-cannot-prove-elapsed-time",
        aged("2026-09-24", "2026-10-01", 7),
        "invalid publication_date",
    )
    reject(
        "negative:elapsed-518401",
        aged("2026-09-24T23:59:59Z", "2026-10-01T00:00:00Z", 6),
        "age under 7",
    )
    reject(
        "negative:elapsed-518401-stored-as-7",
        aged("2026-09-24T23:59:59Z", "2026-10-01T00:00:00Z", 7),
        "age under 7",
    )
    reject(
        "negative:cutoff-plus-one-second",
        aged("2026-09-23T23:48:22Z", "2026-09-30T23:48:21Z", 6),
        "age under 7",
    )
    reject(
        "negative:short-by-one-microsecond",
        aged("2026-09-01T00:00:00.000001Z", "2026-09-08T00:00:00Z", 6),
        "age under 7",
    )
    reject(
        "negative:microsecond-short-stored-as-7",
        aged("2026-09-01T00:00:00.000001Z", "2026-09-08T00:00:00Z", 7),
        "does not match the dates",
    )
    accept("control:exact-604800", aged("2026-09-01T00:00:00Z", "2026-09-08T00:00:00Z", 7))
    accept("control:elapsed-604801-floor-7", aged("2026-09-01T00:00:00Z", "2026-09-08T00:00:01Z", 7))
    accept(
        "control:fractional-second-still-at-least-seven-days",
        aged("2026-09-01T00:00:00Z", "2026-09-08T00:00:00.000001Z", 7),
    )
    accept(
        "control:baseline-thiserror-recomputed-floor",
        aged("2026-09-23T20:24:07.771104Z", "2026-09-30T23:48:21Z", 7),
    )
    accept(
        "control:rfc3339-numeric-offset",
        aged("2026-01-01T00:00:00+00:00", "2026-02-01T00:00:00+00:00", 31),
    )
    accept(
        "control:leap-day-2024",
        aged("2024-02-29T00:00:00Z", "2024-03-08T00:00:00Z", 8),
    )
    reject(
        "negative:baseline-float-age",
        aged("2026-09-23T20:24:07.771104Z", "2026-09-30T23:48:21Z", 7.142),
        "age_days_at_adoption",
    )
    reject("negative:naive-timestamp", aged("2026-09-01T00:00:00", "2026-09-08T00:00:00Z", 7), "invalid publication_date")
    reject(
        "negative:nonzero-offset",
        aged("2026-09-01T00:00:00+01:00", "2026-09-08T00:00:00Z", 7),
        "invalid publication_date",
    )
    reject(
        "negative:adoption-before-publication",
        aged("2026-09-08T00:00:00Z", "2026-09-01T00:00:00Z", 0),
        "adoption_date precedes publication_date",
    )
    for name, bad_age in (
        ("bool-true", True),
        ("float-seven", 7.0),
        ("nan", float("nan")),
        ("inf", float("inf")),
        ("negative-inf", float("-inf")),
    ):
        reject(f"negative:age-{name}", aged("2026-01-01T00:00:00Z", "2026-01-08T00:00:00Z", bad_age), "age_days_at_adoption")
    noon = datetime(2026, 10, 1, 12, 0, tzinfo=timezone.utc)
    later_same_day = aged("2026-09-24T18:00:00Z", "2026-10-01T18:00:00Z", 7)
    later_errors = audited_record_errors(later_same_day, as_of=noon, appointed_names=[])
    results.append(("negative:same-calendar-day-future-adoption", _expect(later_errors, "future adoption_date")))
    midnight = datetime(2026, 10, 1, 0, 0, tzinfo=timezone.utc)
    after_midnight = aged("2026-09-24T00:00:01Z", "2026-10-01T00:00:01Z", 7)
    midnight_errors = audited_record_errors(after_midnight, as_of=midnight, appointed_names=[])
    results.append(("negative:midnight-as-of-does-not-truncate", _expect(midnight_errors, "future adoption_date")))
    date_as_of = audited_record_errors(aged("2026-09-01T00:00:00Z", "2026-09-08T00:00:00Z", 7), as_of=date(2026, 10, 1))
    results.append(
        (
            "negative:date-as-of-cannot-authorize",
            None if date_as_of == ["as_of must be an aware UTC datetime"] else f"unexpected {date_as_of}",
        )
    )
    ahead = (datetime.now(timezone.utc) + timedelta(days=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
    blessed = audited_record_errors(
        aged("2020-01-01T00:00:00Z", ahead, 7),
        as_of=datetime(2099, 1, 1, tzinfo=timezone.utc),
        appointed_names=[],
    )
    results.append(("negative:future-as-of-does-not-bless-after-now", _expect(blessed, "future adoption_date")))

    def sourced(source: str) -> dict:
        return synthetic_audited(
            package={"license_spdx": "MIT", "license_evidence": [{"spdx": "MIT", "source": source}]}
        )

    accept("control:license-relative-path", sourced("vendor/example/LICENSE"))
    accept("control:license-absolute-notice", sourced("/opt/vendor/example/NOTICE"))
    accept("control:license-licence-txt", sourced("LICENCE.txt"))
    accept("control:license-copying", sourced("COPYING"))
    accept(
        "control:license-https-locator",
        sourced("https://example.invalid/crates/example-synthetic/LICENSE-Apache-2.0"),
    )
    for name, source in (
        ("manifest-phrase", "the manifest license field"),
        ("no-notice-phrase", "no notice was read"),
        ("traversal", "../LICENSE"),
        ("nested-traversal", "vendor/../../LICENSE"),
        ("file-url", "file:///tmp/LICENSE"),
        ("credential-url", "https://user:pw@127.0.0.1/LICENSE"),
        ("http-url", "http://example.invalid/LICENSE"),
        ("prose", "see LICENSE"),
        ("manifest-word", "manifest"),
    ):
        reject(f"negative:license-source-{name}", sourced(source), "not LICENSE evidence")

    def weak_duty(duty: str) -> dict:
        return synthetic_audited(
            package={
                "license_spdx": "LGPL-2.1-only",
                "license_evidence": license_evidence("LGPL-2.1-only"),
                "weak_copyleft_review": {
                    "reviewer_name": "Example Reviewer",
                    "reviewer_role": "supply-chain-owner",
                    "obligations": [duty],
                    "evidence": "evidence/synthetic/lgpl-review.md",
                    "scope": ["LGPL-2.1-only"],
                },
            }
        )

    for index, duty in enumerate(
        ("n/a", " N/A ", "na", "none", "pending", "unknown", "not-reviewed", "not applicable", "yes", "no", "true", "  ")
    ):
        reject(f"negative:weak-obligation-{index}", weak_duty(duty), "weak-copyleft obligations")
    reject(
        "negative:and-obligation-na",
        synthetic_audited(
            package={
                "license_spdx": "MIT AND Unicode-3.0",
                "license_evidence": license_evidence("MIT", "Unicode-3.0"),
                "license_obligations": [
                    {"spdx": "MIT", "obligations": ["Retain the copyright and permission notice."]},
                    {"spdx": "Unicode-3.0", "obligations": ["n/a"]},
                ],
            }
        ),
        "obligations do not cover Unicode-3.0",
    )
    return results


def observed_clock(now: datetime) -> datetime:
    """Return an aware UTC datetime. A date or a naive time is not a clock reading."""

    if not isinstance(now, datetime) or now.tzinfo is None or now.utcoffset() != timedelta(0):
        raise ValueError("observed clock requires an aware UTC datetime")
    return now.astimezone(timezone.utc)
