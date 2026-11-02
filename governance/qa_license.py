"""SPDX expression admission for a default-dependency record.

Governance document QA only. ``license_evidence[].source`` is checked as an
artifact locator (a license-file path or an absolute https URL). This module
does not open that path, fetch that URL, download a package, hash an upstream
LICENSE, or run cargo audit, cargo deny, or an image or native scan. A locator
that parses is not evidence that the file exists.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from urllib.parse import unquote, urlsplit

# OSI-approved permissive identifiers this policy will admit on their own.
# BSL-1.0 is the Boost Software License. It is not BUSL-1.1.
PERMISSIVE = frozenset(
    {
        "0BSD",
        "AFL-3.0",
        "Apache-2.0",
        "Artistic-2.0",
        "BSD-1-Clause",
        "BSD-2-Clause",
        "BSD-2-Clause-Patent",
        "BSD-3-Clause",
        "BSL-1.0",
        "ISC",
        "MIT",
        "MIT-0",
        "NCSA",
        "PostgreSQL",
        "Unicode-3.0",
        "Unicode-DFS-2016",
        "Unlicense",
        "Zlib",
    }
)

# Weak copyleft. Admissible only with a review object whose scope lists the id.
WEAK = frozenset(
    {
        "CDDL-1.0",
        "CDDL-1.1",
        "CPL-1.0",
        "EPL-1.0",
        "EPL-2.0",
        "LGPL-2.0-only",
        "LGPL-2.0-or-later",
        "LGPL-2.1",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "LGPL-3.0-only",
        "LGPL-3.0-or-later",
        "MPL-1.1",
        "MPL-2.0",
    }
)

# Strong copyleft. Out of the default artifact even inside OR, even if §3
# does not name it, and even when prohibited_license is false.
STRONG = frozenset(
    {
        "EUPL-1.1",
        "EUPL-1.2",
        "GPL-1.0-only",
        "GPL-1.0-or-later",
        "GPL-2.0",
        "GPL-2.0-only",
        "GPL-2.0-or-later",
        "GPL-3.0",
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "OSL-1.0",
        "OSL-2.0",
        "OSL-2.1",
        "OSL-3.0",
        "RPSL-1.0",
    }
)

# Policy §3. Any token blocks the whole default record.
PROHIBITED = frozenset(
    {
        "AGPL-3.0",
        "AGPL-3.0-only",
        "AGPL-3.0-or-later",
        "BUSL-1.1",
        "Elastic-2.0",
        "SSPL-1.0",
    }
)

# Known licenses that are not OSI-approved permissive. Fail closed.
NON_OSI = frozenset(
    {
        "Beerware",
        "BSD-4-Clause",
        "BSD-4-Clause-UC",
        "CC0-1.0",
        "JSON",
        "OpenSSL",
        "WTFPL",
    }
)

REVIEW_ROLES = frozenset(
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

_TOKEN = re.compile(r"\s+|AND|OR|WITH|[A-Za-z0-9][A-Za-z0-9.+-]*|[()]")
_LICENSE_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9.-]*")
_LICENSE_STEMS = ("LICENSE", "LICENCE", "COPYING", "NOTICE")
_LICENSE_EXTENSIONS = frozenset({"html", "markdown", "md", "rst", "txt"})
_HYPHEN_TAIL = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+-]*")
_PLACEHOLDER_OBLIGATIONS = frozenset(
    {
        "",
        "n a",
        "na",
        "no",
        "none",
        "not applicable",
        "not reviewed",
        "pending",
        "true",
        "unknown",
        "yes",
    }
)


class ParseFailure(Exception):
    def __init__(self, message: str) -> None:
        self.message = message
        super().__init__(message)


@dataclass(frozen=True)
class LicId:
    value: str


@dataclass(frozen=True)
class Conj:
    op: str
    terms: tuple


def parse_expression(text: object) -> LicId | Conj:
    if not isinstance(text, str) or not text.strip():
        raise ParseFailure("license expression is empty")
    if text != text.strip() or "  " in text:
        # Internal spacing is normalized later; reject only unparsed forms below.
        pass
    tokens = _tokenize(text)
    parser = _Parser(tokens)
    tree = parser.parse_or()
    if parser.peek() is not None:
        raise ParseFailure("unsupported or trailing license expression")
    return tree


def _tokenize(text: str) -> list[str]:
    tokens: list[str] = []
    position = 0
    while position < len(text):
        match = _TOKEN.match(text, position)
        if match is None:
            snippet = text[position : position + 24]
            raise ParseFailure(f"unsupported license expression near {snippet!r}")
        token = match.group()
        position = match.end()
        if token.isspace():
            continue
        if token == "WITH" or "+" in token:
            raise ParseFailure("unsupported license expression: WITH and + are not accepted")
        tokens.append(token)
    if not tokens:
        raise ParseFailure("license expression is empty")
    return tokens


class _Parser:
    def __init__(self, tokens: list[str]) -> None:
        self.tokens = tokens
        self.index = 0

    def peek(self) -> str | None:
        if self.index >= len(self.tokens):
            return None
        return self.tokens[self.index]

    def pop(self) -> str:
        token = self.peek()
        if token is None:
            raise ParseFailure("unsupported or truncated license expression")
        self.index += 1
        return token

    def parse_or(self) -> LicId | Conj:
        terms = [self.parse_and()]
        while self.peek() == "OR":
            self.pop()
            terms.append(self.parse_and())
        if len(terms) == 1:
            return terms[0]
        return Conj("OR", tuple(terms))

    def parse_and(self) -> LicId | Conj:
        terms = [self.parse_primary()]
        while self.peek() == "AND":
            self.pop()
            terms.append(self.parse_primary())
        if len(terms) == 1:
            return terms[0]
        return Conj("AND", tuple(terms))

    def parse_primary(self) -> LicId | Conj:
        token = self.pop()
        if token == "(":
            node = self.parse_or()
            if self.pop() != ")":
                raise ParseFailure("unbalanced parentheses in license expression")
            return node
        if token in {"AND", "OR", ")"} or not _LICENSE_ID.fullmatch(token):
            raise ParseFailure(f"unsupported license expression near {token!r}")
        return LicId(token)


def declared_ids(tree: LicId | Conj) -> list[str]:
    if isinstance(tree, LicId):
        return [tree.value]
    found: list[str] = []
    for term in tree.terms:
        found.extend(declared_ids(term))
    return found


def canonical(tree: LicId | Conj) -> str:
    if isinstance(tree, LicId):
        return tree.value
    parts: list[str] = []
    for term in tree.terms:
        text = canonical(term)
        if isinstance(term, Conj) and term.op != tree.op:
            text = f"({text})"
        parts.append(text)
    joiner = " AND " if tree.op == "AND" else " OR "
    return joiner.join(parts)


def _or_nodes(tree: LicId | Conj) -> list[Conj]:
    found: list[Conj] = []

    def walk(node: LicId | Conj) -> None:
        if isinstance(node, LicId):
            return
        if node.op == "OR":
            found.append(node)
        for term in node.terms:
            walk(term)

    walk(tree)
    return found


def _norm_choice(text: str) -> str:
    compacted = re.sub(r"\s+", " ", text.strip())
    while compacted.startswith("(") and compacted.endswith(")") and _parens_wrap(compacted):
        compacted = compacted[1:-1].strip()
    return compacted


def _parens_wrap(text: str) -> bool:
    depth = 0
    for index, char in enumerate(text):
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0 and index != len(text) - 1:
                return False
            if depth < 0:
                return False
    return depth == 0


def effective_ids(tree: LicId | Conj, choice: object) -> tuple[list[str], str | None]:
    nodes = _or_nodes(tree)
    if len(nodes) > 1:
        return [], "unsupported license expression: more than one OR"
    if not nodes:
        if choice not in (None, ""):
            return [], "dual_license_choice is set but the expression has no OR"
        return declared_ids(tree), None
    if not isinstance(choice, str) or not choice.strip():
        return [], "dual_license_choice required"
    wanted = _norm_choice(choice)
    chosen = None
    for term in nodes[0].terms:
        if _norm_choice(canonical(term)) == wanted:
            chosen = term
            break
    if chosen is None:
        return [], "dual_license_choice is not an alternative of the expression"
    hidden = set(declared_ids(nodes[0])) - set(declared_ids(chosen))
    effective = [item for item in declared_ids(tree) if item not in hidden]
    return effective, None


def license_artifact_basename(name: str) -> bool:
    """True for a LICENSE, LICENCE, COPYING, or NOTICE basename.

    A normal suffix may be ``.md``, ``.txt``, ``.rst``, ``.html``, ``.markdown``,
    or a hyphenated tail such as ``-MIT`` or ``-Apache-2.0``. This is a name
    check. It does not open a file or fetch a URL.
    """

    if not name or "/" in name or "\\" in name or any(char.isspace() for char in name):
        return False
    body = name
    if "." in name:
        head, extension = name.rsplit(".", 1)
        if head and extension.lower() in _LICENSE_EXTENSIONS:
            body = head
    for stem in _LICENSE_STEMS:
        if body == stem:
            return True
        prefix = stem + "-"
        if body.startswith(prefix) and _HYPHEN_TAIL.fullmatch(body[len(prefix) :]):
            return True
    return False


def _decode_locator(value: str) -> str:
    seen = value
    for _ in range(3):
        decoded = unquote(seen)
        if decoded == seen:
            return seen
        seen = decoded
    return seen


def license_source_problem(source: object) -> str | None:
    """Return why ``source`` is not an artifact locator.

    ``None`` means the string is a safe relative path, a safe absolute path, or
    an absolute https URL whose basename is a license artifact name. It does
    not mean that path or URL was opened, and it does not mean the bytes exist.
    """

    if not isinstance(source, str) or not source or source != source.strip():
        return "unrecognized license artifact locator"
    if any(char.isspace() for char in source) or "\\" in source or "\x00" in source:
        return "unrecognized license artifact locator"
    if source.startswith("https://"):
        return _https_locator_problem(source)
    if "://" in source or source.lower().startswith("file:"):
        return "unrecognized license artifact locator"
    return _path_locator_problem(source)


def _path_locator_problem(source: str) -> str | None:
    if ":" in source or "%" in source:
        return "unrecognized license artifact locator"
    parts = source.split("/")
    if source.startswith("/"):
        parts = parts[1:]
    if not parts or any(part in {"", ".", ".."} for part in parts):
        return "ambiguous license artifact locator"
    if license_artifact_basename(parts[-1]):
        return None
    return "unrecognized license artifact locator"


def _https_locator_problem(source: str) -> str | None:
    split = urlsplit(source)
    if split.scheme != "https" or not split.hostname:
        return "unrecognized license artifact locator"
    if split.username is not None or split.password is not None or "@" in split.netloc:
        return "unrecognized license artifact locator"
    path = _decode_locator(split.path)
    if any(char.isspace() for char in path) or "\\" in path or "\x00" in path or "%" in path:
        return "ambiguous license artifact locator"
    parts = path.split("/")
    body = parts[1:] if parts and parts[0] == "" else parts
    if not body or any(part in {"", ".", ".."} for part in body):
        return "ambiguous license artifact locator"
    if license_artifact_basename(body[-1]):
        return None
    return "unrecognized license artifact locator"


def obligation_is_placeholder(value: object) -> bool:
    """True for empty text and non-obligations such as n/a, none, or not reviewed.

    Letter case, surrounding whitespace, and ``-``, ``/``, or ``.`` separators
    do not make a placeholder into a duty. A sentence that merely contains one
    of those words is not a placeholder.
    """

    if not isinstance(value, str):
        return True
    folded = value.casefold()
    folded = re.sub(r"[/_.\-]+", " ", folded)
    folded = re.sub(r"\s+", " ", folded).strip()
    return folded in _PLACEHOLDER_OBLIGATIONS


def obligation_text_ok(value: object) -> bool:
    return not obligation_is_placeholder(value)


def classify(token: str) -> str:
    if token in PROHIBITED:
        return "prohibited"
    if token in STRONG:
        return "strong"
    if token.startswith("LicenseRef-") or token.startswith("DocumentRef-"):
        return "unknown"
    if token in NON_OSI:
        return "non-osi"
    if token in PERMISSIVE:
        return "permissive"
    if token in WEAK:
        return "weak"
    return "unknown"


def _evidence_errors(label: str, ids: list[str], evidence: object) -> list[str]:
    errors: list[str] = []
    if not isinstance(evidence, list) or not evidence:
        return [f"{label}: license_evidence"]
    covered: set[str] = set()
    for index, item in enumerate(evidence):
        if not isinstance(item, dict):
            errors.append(f"{label}: license_evidence[{index}] must name the LICENSE file")
            continue
        spdx = item.get("spdx")
        source = item.get("source")
        if not isinstance(spdx, str) or spdx not in ids:
            errors.append(f"{label}: license_evidence[{index}] does not match a declared license")
            continue
        problem = license_source_problem(source)
        if problem is not None:
            errors.append(f"{label}: license_evidence[{index}] source is not LICENSE evidence ({problem})")
            continue
        covered.add(spdx)
    for spdx in ids:
        if spdx not in covered:
            errors.append(f"{label}: license_evidence does not cover {spdx}")
    return errors


def _obligation_errors(label: str, effective: list[str], obligations: object) -> list[str]:
    if len(effective) < 2:
        if obligations not in (None, [], ()):
            if not isinstance(obligations, list):
                return [f"{label}: license_obligations"]
        return []
    if not isinstance(obligations, list):
        return [f"{label}: obligations do not cover {effective[0]}"]
    covered: set[str] = set()
    errors: list[str] = []
    for index, item in enumerate(obligations):
        if not isinstance(item, dict):
            errors.append(f"{label}: license_obligations[{index}]")
            continue
        spdx = item.get("spdx")
        duties = item.get("obligations")
        if not isinstance(spdx, str):
            errors.append(f"{label}: license_obligations[{index}]")
            continue
        if not isinstance(duties, list) or not duties or not all(obligation_text_ok(duty) for duty in duties):
            continue
        covered.add(spdx)
    for spdx in effective:
        if spdx not in covered:
            errors.append(f"{label}: obligations do not cover {spdx}")
    return errors


def _review_errors(label: str, effective_weak: list[str], review: object) -> list[str]:
    if not effective_weak:
        if isinstance(review, bool):
            return [f"{label}: weak-copyleft review must name the reviewer, obligations, and evidence"]
        return []
    if isinstance(review, bool) or not isinstance(review, dict):
        return [f"{label}: weak-copyleft review must name the reviewer, obligations, and evidence"]
    errors: list[str] = []
    name = review.get("reviewer_name")
    role = review.get("reviewer_role")
    duties = review.get("obligations")
    evidence = review.get("evidence")
    scope = review.get("scope")
    if not _person_name(name):
        errors.append(f"{label}: weak-copyleft reviewer_name")
    if role not in REVIEW_ROLES:
        errors.append(f"{label}: weak-copyleft reviewer_role")
    if not isinstance(duties, list) or not duties or not all(obligation_text_ok(item) for item in duties):
        errors.append(f"{label}: weak-copyleft obligations")
    if not isinstance(evidence, str) or "/" not in evidence or evidence.strip().lower() in {"true", "yes", "pending"}:
        errors.append(f"{label}: weak-copyleft evidence")
    if not isinstance(scope, list):
        errors.append(f"{label}: weak-copyleft review scope")
        return errors
    for spdx in effective_weak:
        if spdx not in scope:
            errors.append(f"{label}: weak-copyleft review scope does not cover {spdx}")
    return errors


def _person_name(value: object) -> bool:
    if not isinstance(value, str):
        return False
    parts = value.split()
    if len(parts) < 2:
        return False
    if value.strip() in REVIEW_ROLES or value.strip() == "OWNER_PENDING":
        return False
    return all(len(part) >= 2 and part.isalpha() for part in parts)


def evaluate_license(
    *,
    label: str,
    expression: object,
    choice: object,
    evidence: object,
    obligations: object,
    review: object,
) -> list[str]:
    try:
        tree = parse_expression(expression)
    except ParseFailure as exc:
        return [f"{label}: {exc.message}"]
    ids = declared_ids(tree)
    errors: list[str] = []
    classes = {token: classify(token) for token in ids}
    for token, kind in classes.items():
        if kind == "prohibited":
            errors.append(f"{label}: prohibited license: {token}")
        elif kind == "strong":
            errors.append(f"{label}: strong copyleft: {token}")
        elif kind in {"unknown", "non-osi"}:
            errors.append(f"{label}: license not admissible: {token}")
    effective, choice_error = effective_ids(tree, choice)
    if choice_error:
        errors.append(f"{label}: {choice_error}")
    if errors:
        return errors
    errors.extend(_evidence_errors(label, ids, evidence))
    errors.extend(_obligation_errors(label, effective, obligations))
    weak_ids = [token for token in effective if classes[token] == "weak"]
    errors.extend(_review_errors(label, weak_ids, review))
    return errors
