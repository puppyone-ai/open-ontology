#!/usr/bin/env python3
"""Strip a SHOW GRANTS authenticator before a collector serializes or logs it.

The quoted token after IDENTIFIED BY PASSWORD is replaced with '<redacted>'.
Privilege text, account principals, and surrounding bytes stay. A digit
sequence that is not inside that clause is left alone.
"""

from __future__ import annotations

import sys


MARKER = "IDENTIFIED BY PASSWORD"
REPLACEMENT = "IDENTIFIED BY PASSWORD '<redacted>'"


def redact_authenticator(text: str) -> str:
    if not isinstance(text, str):
        text = text.decode("utf-8", "replace")
    upper = text.upper()
    out = []
    cursor = 0
    while True:
        start = upper.find(MARKER, cursor)
        if start < 0:
            out.append(text[cursor:])
            break
        after = start + len(MARKER)
        scan = after
        while scan < len(text) and text[scan].isspace():
            scan += 1
        if scan >= len(text) or text[scan] not in "'\"":
            out.append(text[cursor:after])
            cursor = after
            continue
        quote = text[scan]
        end = text.find(quote, scan + 1)
        if end < 0:
            out.append(text[cursor:after])
            cursor = after
            continue
        out.append(text[cursor:start])
        out.append(REPLACEMENT)
        cursor = end + 1
    return "".join(out)


def _self_test() -> int:
    raw = (
        "GRANT USAGE ON *.* TO `oo_reader`@`%` IDENTIFIED BY PASSWORD "
        "'*SYNTHETICPLACEHOLDER00000000000000000000'\n"
        "GRANT SELECT ON `cust_east`.* TO `oo_reader`@`%`\n"
        "12345678901234567890.1234567890\n"
    )
    got = redact_authenticator(raw)
    checks = [
        "SYNTHETICPLACEHOLDER" not in got,
        REPLACEMENT in got,
        "GRANT SELECT ON `cust_east`.* TO `oo_reader`@`%`" in got,
        "`oo_reader`@`%`" in got,
        "12345678901234567890.1234567890" in got,
        got.count(REPLACEMENT) == 1,
    ]
    if not all(checks):
        print("redact_auth self-test failed", file=sys.stderr)
        return 1
    print("redact_auth self-test ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(_self_test())
