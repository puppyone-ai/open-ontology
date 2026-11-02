"""Document checks for the governance slice.

Skipped tools stay skipped. A substring that also appears in a negation is not
an operative rule. HTML comments, including a comment that spans lines or never
closes, are not operative text. Contractions such as "isn't" and "cannot" are
negations. The tree scan and the self-test call the same functions. The prefix
list is not a general reading of English.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)\s]+)\)")
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
NEGATION_PREFIX = re.compile(
    r"(?i)(?:"
    r"\bnot\b|\bnever\b|\bno longer\b|\bdoes not\b|\bdo not\b|"
    r"\bdon't\b|\bdoesn't\b|\bisn't\b|\bwasn't\b|\baren't\b|\bwon't\b|\bcan't\b|\bcannot\b|"
    r"\bfalse that\b|\bwithout\b"
    r").{0,80}$"
)
FORM_TYPES = {"markdown", "textarea", "input", "dropdown", "checkboxes"}
FORBIDDEN_CLAIMS = (
    "production-ready",
    "production ready",
    "ready for production",
    "all tests pass",
    "gvisor is installed",
    "gvisor enabled",
    "cdc is running",
    "cdc works",
    "mcp server is available",
    "mcp is available",
    "all dependencies are compliant",
    "every dependency is apache",
)

GOVERNANCE_FILES = [
    "LICENSE",
    "NOTICE",
    "README.md",
    "governance/CONTRIBUTING.md",
    "governance/CODE_OF_CONDUCT.md",
    "governance/SECURITY.md",
    "governance/SUPPORT.md",
    ".github/CODEOWNERS",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/ISSUE_TEMPLATE/bug.yml",
    ".github/ISSUE_TEMPLATE/task.yml",
    ".github/ISSUE_TEMPLATE/dependency.yml",
    "governance/README.md",
    "governance/adr-template.md",
    "governance/ownership-map.md",
    "governance/review-guidance.md",
    "governance/supported-matrix.md",
    "governance/check_governance.py",
    "governance/qa_license.py",
    "governance/qa_record.py",
    "governance/qa_docs.py",
    "supply-chain/policy.md",
    "supply-chain/dependency-audit-record.template.json",
    "examples/README.md",
    "examples/governance/synth-typed-file.md",
]

CLAIM_FILES = [
    "README.md",
    "NOTICE",
    "governance/SECURITY.md",
    "governance/SUPPORT.md",
    "governance/CONTRIBUTING.md",
    "examples/README.md",
    "examples/governance/synth-typed-file.md",
    "governance/supported-matrix.md",
    "governance/README.md",
    "supply-chain/policy.md",
]

# Full sentences. Deleting the sentence fails even if a negated line keeps a fragment.
OPERATIVE_RULES = [
    (
        "governance/SECURITY.md",
        "Do not open a public issue, pull request, or discussion for a suspected vulnerability.",
    ),
    ("governance/SECURITY.md", "This is a release blocker."),
    (
        "governance/SECURITY.md",
        "A metadata-only mode still authenticates, redacts, and audits, and it does not dump the catalog.",
    ),
    (
        "governance/SECURITY.md",
        "The design requires an admission audit record before a result is returned, a hash chain so tampering is detectable, and a collector that is not a general write account on the control database.",
    ),
    (
        "governance/SECURITY.md",
        "`REQ-SEC-02` and [docs/10 §6](../docs/10-security.md) are the future bar for output handling: no external images, frames, or scripts from an answer; no automatic link previews; CSV and Excel cells that could be formulas get a protective prefix, including cells whose danger is only leading whitespace or a control character.",
    ),
    (
        "governance/SECURITY.md",
        "`REQ-SEC-03` describes emergency stop: close new sessions and data admission, revoke session and external tokens, cancel queries, drop proxy mappings, isolate and destroy sandboxes.",
    ),
    ("supply-chain/policy.md", "AGPL, BSL, ELv2, and SSPL do not enter the default package."),
    (
        "supply-chain/policy.md",
        "A prohibited token from §3 blocks the default record even when it is only one side of an `OR`, and even when `prohibited_license` is false.",
    ),
    (
        "supply-chain/policy.md",
        "A version published fewer than 7 days before adoption does not enter the default baseline.",
    ),
    (
        "supply-chain/policy.md",
        "Nobody is appointed while `.github/CODEOWNERS` still says `OWNER_PENDING` and SECURITY.md still says the security owner is not appointed.",
    ),
    (
        "supply-chain/policy.md",
        "An audited record carries an advisory block.",
    ),
    (
        "supply-chain/policy.md",
        "A known exploitable high-severity issue cannot be waived by this block, by `waive`, or by a ledger note.",
    ),
    (
        "supply-chain/policy.md",
        "The governance checker validates that shape. It does not run `cargo audit`, `cargo deny`, or an image or native scanner.",
    ),
    ("governance/CONTRIBUTING.md", "That expansion is the only permitted rewrite."),
    (
        "governance/CONTRIBUTING.md",
        "R9 still judges an unfiltered `SELECT *` on a large table by the original star.",
    ),
    ("governance/CONTRIBUTING.md", "Expansion does not bypass that rejection."),
    (
        "governance/CODE_OF_CONDUCT.md",
        "The project-specific changes are the Enforcement section and a preface on the Enforcement Guidelines.",
    ),
    ("NOTICE", "https://creativecommons.org/licenses/by/4.0/"),
    ("NOTICE", "It does not grant rights in third-party code, data, or marks."),
    (
        "governance/supported-matrix.md",
        "Formula-safe CSV and Excel output is an SP05-W06 requirement (`AT-FILE-05`, `AT-SEC-03`).",
    ),
    (
        "governance/supported-matrix.md",
        "Answer HTML and Markdown rendering is an SP11 requirement (`REQ-SEC-02`, `AT-SEC-03`).",
    ),
    (
        "governance/README.md",
        "Cargo.lock is in the tree. It is not a license clearance and it is not a vulnerability scan.",
    ),
    (
        "governance/README.md",
        "`crates/oo-arch-check` is a workspace dependency-direction gate, and its crate description says it is not a product service.",
    ),
    (
        "governance/README.md",
        "Implementation and CI presence are not evidence of full product acceptance.",
    ),
    ("examples/README.md", "No program in this repository loads this sample."),
    ("examples/governance/synth-typed-file.md", "This sample is not evidence that a product command runs."),
    (
        ".github/ISSUE_TEMPLATE/task.yml",
        "Product work cites both an existing REQ- id and an existing AT- id.",
    ),
    (".github/ISSUE_TEMPLATE/bug.yml", "governance-only: no product id"),
    (
        ".github/ISSUE_TEMPLATE/dependency.yml",
        "Admission follows supply-chain/policy.md sections 1 through 4, including OSI-approved permissive licenses, reviewed weak copyleft, prohibited tokens inside OR expressions, strong copyleft, unknown LicenseRef identifiers, the seven-day rule, and the advisory disposition.",
    ),
]

WORK_ITEM_FIELDS = (
    "req_id",
    "at_id",
    "dependencies",
    "target_crate",
    "expected_files",
    "failure",
    "owner_role",
    "reviewer_role",
)


def strip_fences(text: str) -> str:
    kept: list[str] = []
    fenced = False
    for line in text.splitlines(True):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if not fenced:
            kept.append(line)
    return "".join(kept)


def _line_start(text: str, index: int) -> int:
    return text.rfind("\n", 0, index) + 1


def _fence_at(text: str, index: int) -> bool:
    if not text.startswith("```", index):
        return False
    return text[_line_start(text, index) : index].strip() == ""


def _after_line(text: str, index: int) -> int:
    newline = text.find("\n", index)
    if newline < 0:
        return len(text)
    return newline + 1


def visible_prose(text: str) -> str:
    """Text with fenced code and HTML comments removed.

    A closed comment is removed, including a comment that spans lines. An
    unclosed ``<!--`` hides the rest of the text, so the missing closer cannot
    leave hidden prose in force. Comment markers inside a fence stay inside
    the fence. This filter is conservative. It is not a proof of what the
    surrounding sentences mean.
    """

    out: list[str] = []
    index = 0
    length = len(text)
    in_fence = False
    while index < length:
        if in_fence:
            if _fence_at(text, index):
                in_fence = False
                index = _after_line(text, index)
                continue
            newline = text.find("\n", index)
            if newline < 0:
                break
            index = newline + 1
            continue
        if text.startswith("<!--", index):
            close = text.find("-->", index + 4)
            if close < 0:
                break
            index = close + 3
            continue
        if _fence_at(text, index):
            in_fence = True
            index = _after_line(text, index)
            continue
        out.append(text[index])
        index += 1
    return "".join(out)


def whitespace_errors_bytes(data: bytes, label: str) -> list[str]:
    errors: list[str] = []
    if b"\r" in data:
        errors.append(f"{label}: CR byte")
    if b"\0" in data:
        errors.append(f"{label}: NUL byte")
    for number, line in enumerate(data.splitlines(), 1):
        if line.endswith(b" ") or line.endswith(b"\t"):
            errors.append(f"{label}:{number}: trailing whitespace")
        if line.startswith(b"<<<<<<<") or line.startswith(b">>>>>>>") or line == b"=======":
            errors.append(f"{label}:{number}: conflict marker")
    return errors


def whitespace_errors(path: Path) -> list[str]:
    return whitespace_errors_bytes(path.read_bytes(), str(path))


def codeowners_active_lines(text: str) -> list[int]:
    active: list[int] = []
    for number, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            active.append(number)
    return active


def claim_hits(text: str) -> list[str]:
    lowered = text.lower()
    return [phrase for phrase in FORBIDDEN_CLAIMS if phrase in lowered]


def operative_holds(text: str, sentence: str) -> bool:
    """True when sentence appears in visible prose, outside a negation prefix.

    HTML comments and fenced code are not visible. Contractions such as
    "isn't" and "cannot", and the older prefixes (not, never, don't), keep the
    sentence from counting. A negation in an earlier sentence does not hide a
    later one. This is a fixed prefix list, not a general reading of English.
    """

    visible = visible_prose(text)
    start = 0
    while True:
        index = visible.find(sentence, start)
        if index < 0:
            return False
        line_start = visible.rfind("\n", 0, index) + 1
        prefix = visible[line_start:index]
        boundary = max(prefix.rfind(". "), prefix.rfind("? "), prefix.rfind("! "))
        if boundary >= 0:
            prefix = prefix[boundary + 2 :]
        if NEGATION_PREFIX.search(prefix) is None:
            return True
        start = index + len(sentence)


def local_link_errors(text: str, path: Path) -> list[str]:
    """Missing relative markdown targets. Tree scan and self-test both use this."""

    missing: list[str] = []
    for target in LINK_RE.findall(strip_fences(text)):
        if target.startswith(("http://", "https://", "mailto:")):
            continue
        cleaned = target.split("#", 1)[0]
        if not cleaned:
            continue
        resolved = (path.parent / cleaned).resolve()
        if not (resolved.is_file() or resolved.is_dir()):
            missing.append(f"{path}: {target}")
    return missing


def classify_diff_check(untracked_paths: list[str]) -> str:
    """git diff --check does not read untracked files. That case is skipped."""

    if untracked_paths:
        return "skipped"
    return "applicable"


def form_errors(path: Path, document: object) -> list[str]:
    errors: list[str] = []
    if not isinstance(document, dict):
        return [f"{path}: root is not a mapping"]
    if path.name == "config.yml":
        if document.get("blank_issues_enabled") is not False:
            errors.append(f"{path}: blank_issues_enabled")
        if "contact_links" in document:
            errors.append(f"{path}: contact_links would need a real URL")
        return errors
    for key in ("name", "description", "body"):
        if key not in document:
            errors.append(f"{path}: missing {key}")
    body = document.get("body")
    if not isinstance(body, list) or not body:
        return errors + [f"{path}: body"]
    seen: set[str] = set()
    for index, item in enumerate(body):
        if not isinstance(item, dict):
            errors.append(f"{path}: body[{index}]")
            continue
        kind = item.get("type")
        if kind not in FORM_TYPES:
            errors.append(f"{path}: body[{index}] type {kind}")
            continue
        if kind == "markdown":
            value = (item.get("attributes") or {}).get("value")
            if not isinstance(value, str) or not value.strip():
                errors.append(f"{path}: empty markdown")
            continue
        item_id = item.get("id")
        if not isinstance(item_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", item_id):
            errors.append(f"{path}: bad id at {index}")
        elif item_id in seen:
            errors.append(f"{path}: duplicate id {item_id}")
        else:
            seen.add(item_id)
        attributes = item.get("attributes") or {}
        if not attributes.get("label"):
            errors.append(f"{path}: {item_id} label")
        if kind == "dropdown" and not attributes.get("options"):
            errors.append(f"{path}: {item_id} options")
        if kind == "checkboxes":
            options = attributes.get("options")
            if not isinstance(options, list) or not options:
                errors.append(f"{path}: {item_id} options")
            elif not any(option.get("required") is True for option in options if isinstance(option, dict)):
                errors.append(f"{path}: {item_id} has no required confirmation")
    errors.extend(work_item_field_errors(path, document))
    errors.extend(bug_form_errors(path, document))
    errors.extend(dependency_form_errors(path, document))
    return errors


def work_item_field_errors(path: Path, document: object) -> list[str]:
    if path.name != "task.yml" or not isinstance(document, dict):
        return []
    body = document.get("body")
    if not isinstance(body, list):
        return [f"{path}: body"]
    found: dict[str, dict] = {}
    for item in body:
        if isinstance(item, dict) and isinstance(item.get("id"), str):
            found[item["id"]] = item
    errors: list[str] = []
    for item_id in WORK_ITEM_FIELDS:
        item = found.get(item_id)
        if item is None:
            errors.append(f"{path}: missing {item_id}")
            continue
        if (item.get("validations") or {}).get("required") is not True:
            errors.append(f"{path}: {item_id} is not required")
    if "work_class" not in found:
        errors.append(f"{path}: missing work_class")
    return errors


def _required_ids(path: Path, document: object, ids: tuple[str, ...]) -> list[str]:
    if not isinstance(document, dict) or not isinstance(document.get("body"), list):
        return [f"{path}: body"]
    found: dict[str, dict] = {}
    for item in document["body"]:
        if isinstance(item, dict) and isinstance(item.get("id"), str):
            found[item["id"]] = item
    errors: list[str] = []
    for item_id in ids:
        item = found.get(item_id)
        if item is None:
            errors.append(f"{path}: missing {item_id}")
        elif (item.get("validations") or {}).get("required") is not True:
            errors.append(f"{path}: {item_id} is not required")
    return errors


def bug_form_errors(path: Path, document: object) -> list[str]:
    if path.name != "bug.yml":
        return []
    return _required_ids(path, document, ("ids",))


def dependency_form_errors(path: Path, document: object) -> list[str]:
    if path.name != "dependency.yml":
        return []
    return _required_ids(
        path,
        document,
        ("ecosystem", "name", "source", "published", "license", "features", "seven_day", "advisory", "default_features"),
    )


def load_yaml(path: Path) -> object:
    completed = subprocess.run(
        ["ruby", "-ryaml", "-e", "require 'json'; print JSON.generate(YAML.safe_load(File.read(ARGV[0])))", str(path)],
        text=True,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "ruby yaml failed")
    return json.loads(completed.stdout)


def self_test(root: Path) -> list[tuple[str, str | None]]:
    results: list[tuple[str, str | None]] = []
    security = (root / "governance/SECURITY.md").read_text()
    sentence = "The design requires an admission audit record before a result is returned, a hash chain so tampering is detectable, and a collector that is not a general write account on the control database."
    if not operative_holds(security, sentence):
        results.append(("operative:real-hash-chain-sentence", "real SECURITY.md lost the sentence"))
    else:
        results.append(("operative:real-hash-chain-sentence", None))
    removed = security.replace(sentence, "This file does not implement a hash chain.")
    if "hash chain" not in removed or operative_holds(removed, sentence):
        results.append(("operative:deleted-sentence-still-holds", "deletion was not detected"))
    else:
        results.append(("operative:deleted-sentence-still-holds", None))
    negated = security.replace(sentence, "It is not the case that " + sentence)
    if operative_holds(negated, sentence):
        results.append(("operative:negated-sentence-counts", "negation was accepted"))
    else:
        results.append(("operative:negated-sentence-counts", None))
    followed = "This note does not retire the rule. " + sentence
    if not operative_holds(followed, sentence):
        results.append(("operative:earlier-negation-hides-rule", "a real following sentence was rejected"))
    else:
        results.append(("operative:earlier-negation-hides-rule", None))
    catalog = "A metadata-only mode still authenticates, redacts, and audits, and it does not dump the catalog."
    catalog_only = "This file does not dump the catalog."
    if operative_holds(catalog_only, catalog):
        results.append(("operative:fragment-is-not-the-rule", "fragment matched"))
    else:
        results.append(("operative:fragment-is-not-the-rule", None))
    comment_cases = (
        ("operative:html-comment-counts", "<!-- " + sentence + " -->"),
        ("operative:multiline-comment-counts", "<!--\n" + sentence + "\n-->\n"),
        ("operative:unclosed-comment-counts", "Before.\n<!--\n" + sentence + "\n"),
        ("operative:fenced-sentence-counts", "```\n" + sentence + "\n```\n"),
        ("operative:isnt-prefix-counts", "This isn't the rule: " + sentence),
        ("operative:wasnt-prefix-counts", "This wasn't the rule: " + sentence),
        ("operative:arent-prefix-counts", "These aren't the rule: " + sentence),
        ("operative:wont-prefix-counts", "This won't be the rule: " + sentence),
        ("operative:cant-prefix-counts", "This can't be the rule: " + sentence),
        ("operative:cannot-prefix-counts", "This cannot be the rule: " + sentence),
    )
    for name, sample in comment_cases:
        if operative_holds(sample, sentence):
            results.append((name, "hidden or negated sentence was accepted"))
        else:
            results.append((name, None))
    if not operative_holds("<!-- retired -->\n" + sentence, sentence):
        results.append(("operative:closed-comment-hides-following-rule", "text after a closed comment was dropped"))
    else:
        results.append(("operative:closed-comment-hides-following-rule", None))
    if not operative_holds("This isn't a retirement. " + sentence, sentence):
        results.append(("operative:contraction-in-earlier-sentence", "a later sentence was rejected"))
    else:
        results.append(("operative:contraction-in-earlier-sentence", None))
    if operative_holds("```\n<!--\n" + sentence + "\n```\n" + sentence, sentence) is not True:
        results.append(("operative:comment-inside-fence-does-not-eat-the-rule", "prose after a fenced comment was lost"))
    else:
        results.append(("operative:comment-inside-fence-does-not-eat-the-rule", None))

    sample = "See [missing](does-not-exist.md).\n"
    missing = local_link_errors(sample, root / "README.md")
    if missing != [f"{root / 'README.md'}: does-not-exist.md"]:
        results.append(("links:same-function-sees-missing", str(missing)))
    else:
        results.append(("links:same-function-sees-missing", None))
    for name, target in (("file", "LICENSE"), ("directory", "crates"),
                         ("missing-directory", "does-not-exist/")):
        errors = local_link_errors(f"See [target]({target}).\n", root / "README.md")
        rejected = name == "missing-directory"
        results.append((f"links:{name}", None if bool(errors) == rejected else str(errors)))
    if local_link_errors("See [web](https://example.invalid/x).\n", root / "README.md"):
        results.append(("links:external-skipped", "external link was treated as a file"))
    else:
        results.append(("links:external-skipped", None))

    if classify_diff_check(["governance/check_governance.py"]) != "skipped":
        results.append(("diff:untracked-is-skipped", "untracked diff was applicable"))
    else:
        results.append(("diff:untracked-is-skipped", None))
    if classify_diff_check([]) != "applicable":
        results.append(("diff:tracked-is-applicable", "empty untracked set was skipped"))
    else:
        results.append(("diff:tracked-is-applicable", None))

    dirty = whitespace_errors_bytes(b"kept\nbad \n", "untracked-sample")
    if not any("trailing whitespace" in item for item in dirty):
        results.append(("whitespace:untracked-bytes", str(dirty)))
    else:
        results.append(("whitespace:untracked-bytes", None))
    if whitespace_errors_bytes(b"kept\n", "clean-sample"):
        results.append(("whitespace:clean-bytes", "clean bytes failed"))
    else:
        results.append(("whitespace:clean-bytes", None))

    task = {
        "name": "Work item",
        "description": "x",
        "body": [
            {"type": "markdown", "attributes": {"value": "Product work cites both an existing REQ- id and an existing AT- id."}},
            {"type": "input", "id": "failure", "attributes": {"label": "Failure"}, "validations": {"required": True}},
        ],
    }
    field_errors = work_item_field_errors(Path("task.yml"), task)
    if not any("missing req_id" in item for item in field_errors):
        results.append(("form:removed-req-id", str(field_errors)))
    else:
        results.append(("form:removed-req-id", None))
    return results
