#!/usr/bin/env python3
"""Check the r-efi attribution correction against crate bytes and git scope."""

import hashlib
import subprocess
import sys
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
ISC = ROOT / "contracts/supply-chain/isc"
RAW = ROOT / "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/upstream"
A067 = ROOT / "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067/SP00/isc-policy"

AUTHORS6 = "d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111"
AUTHORS5 = "ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7"
README = "420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1"
CRATE6 = "f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf"
CRATE5 = "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f"
ISC_BODY = "7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4"
RUSTLS_CRATE = "0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634"
ISC_LINE = "Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>"
MIT_LINE = "Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>"
COPYRIGHT = (
    "        Copyright (C) 2017-2023 Red Hat, Inc.\n"
    "        Copyright (C) 2019-2023 Microsoft Corporation\n"
    "        Copyright (C) 2022-2023 David Rheinsberg\n"
)
OLD_NOTICE = (
    "`r-efi` 6.0.0 has no MIT license text in the crate. "
    "Its README names `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. "
    "A distribution that relies on MIT still owes the MIT notice duty and "
    "cannot point at a MIT license file in this crate. `AUTHORS` is not that license."
)
OLD_POLICY_REFI = "That acceptance does not mean the MIT text is in the crate."
OLD_POLICY_FIXTURE = "`fixtures/mit-or-gpl-missing-mit-text` is the synthetic form of the `r-efi` fact."
OLD_HANDOFF = "that is the `r-efi` 6.0.0 situation, and it is not proof the MIT text shipped."
OLD_REPORT_GAP = "`r-efi` 6.0.0 is the real crate with that gap."
OLD_REPORT_COMMA = "rustls `LICENSE-ISC` (Copyright (c) 2016 Joseph Birr-Pixton)"

failures = []


def fail(message):
    failures.append(message)


def sha256(path):
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def require(condition, message):
    if not condition:
        fail(message)


def git(*args):
    result = subprocess.run(
        ["git", "-C", str(ROOT), *args],
        check=False,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        fail(f"git {' '.join(args)} -> {result.returncode}: {result.stderr.strip()}")
    return result.stdout


notice = (ISC / "NOTICE.md").read_text()
policy = (ISC / "policy.md").read_text()
provenance = (ISC / "provenance.md").read_text()
erratum = (ISC / "ERRATUM.md").read_text()
current = notice + "\n" + policy + "\n" + provenance

authors6 = (ISC / "bodies/r-efi-6.0.0/AUTHORS").read_bytes()
authors5 = (ISC / "bodies/r-efi-5.3.0/AUTHORS").read_bytes()
license_isc = (ISC / "bodies/rustls-0.23.45/LICENSE-ISC").read_bytes()
readme6 = (RAW / "r-efi-6.0.0.README.md").read_bytes()
readme5 = (RAW / "r-efi-5.3.0.README.md").read_bytes()

require(sha256(ISC / "bodies/r-efi-6.0.0/AUTHORS") == AUTHORS6, "AUTHORS 6 hash")
require(sha256(ISC / "bodies/r-efi-5.3.0/AUTHORS") == AUTHORS5, "AUTHORS 5 hash")
require(sha256(RAW / "r-efi-6.0.0.AUTHORS") == AUTHORS6, "raw AUTHORS 6")
require(sha256(RAW / "r-efi-5.3.0.AUTHORS") == AUTHORS5, "raw AUTHORS 5")
require(sha256(RAW / "r-efi-6.0.0.README.md") == README, "README 6 hash")
require(sha256(RAW / "r-efi-5.3.0.README.md") == README, "README 5 hash")
require(readme6 == readme5, "README bytes differ")
require(sha256(ISC / "bodies/rustls-0.23.45/LICENSE-ISC") == ISC_BODY, "LICENSE-ISC hash")
require(authors6 == (RAW / "r-efi-6.0.0.AUTHORS").read_bytes(), "body/raw AUTHORS 6")
require(authors5 == (RAW / "r-efi-5.3.0.AUTHORS").read_bytes(), "body/raw AUTHORS 5")
require(license_isc == (RAW / "rustls-0.23.45.LICENSE-ISC").read_bytes(), "body/raw LICENSE-ISC")

recorded = (RAW / "sha256.txt").read_text()
require(CRATE6 in recorded, "crate 6 sha missing from sha256.txt")
require(CRATE5 in recorded, "crate 5 sha missing from sha256.txt")
require(RUSTLS_CRATE in recorded, "rustls crate sha missing from sha256.txt")

grant = b"Permission is hereby granted, free of charge"
require(grant in authors6 and grant in authors5, "MIT grant missing from AUTHORS")
require(grant not in readme6, "README contains the MIT grant")
require(b"AUTHORS-MIT:" in authors6 and b"AUTHORS-ASL:" in authors6 and b"AUTHORS-LGPL:" in authors6, "headings")
require(b"http://www.apache.org/licenses/LICENSE-2.0" in authors6, "Apache pointer")
require(b"http://www.gnu.org/licenses/" in authors6, "LGPL pointer")
require(COPYRIGHT.encode() in authors6 and COPYRIGHT.encode() in authors5, "copyright block in AUTHORS")
require(COPYRIGHT in notice and COPYRIGHT in erratum, "copyright block quoted")

lines6 = authors6.decode().splitlines()
lines5 = authors5.decode().splitlines()
extra = [line for line in lines6 if line not in lines5]
missing = [line for line in lines5 if line not in lines6]
require(extra == ["        Alan Egerton <eggyal@gmail.com>"], f"unexpected 6.0.0 extra lines: {extra}")
require(missing == [], f"5.3.0 lines missing from 6.0.0: {missing}")
require(b"Alan Egerton <eggyal@gmail.com>" not in authors5, "5.3.0 contains Alan Egerton")

isc_lines = license_isc.decode().splitlines()
require(len(isc_lines) >= 2 and isc_lines[1] == ISC_LINE, "LICENSE-ISC second line")
mit_lines = (RAW / "rustls-0.23.45.LICENSE-MIT").read_text().splitlines()
require(mit_lines[0] == MIT_LINE, "LICENSE-MIT first line")
require(ISC_LINE in notice and ISC_LINE in erratum and ISC_LINE in provenance, "comma line cited")
require(MIT_LINE in notice, "MIT no-comma line kept")

for label, text in (("NOTICE", notice), ("policy", policy), ("provenance", provenance), ("ERRATUM", erratum)):
    require(AUTHORS6 in text, f"{label} missing AUTHORS 6 hash")
    require("LICENSE" in text and "COPYING" in text and "NOTICE" in text, f"{label} drops filename fact")

require(CRATE6 in provenance and CRATE6 in erratum, "crate 6 sha")
require(CRATE5 in provenance and CRATE5 in erratum, "crate 5 sha")
require(AUTHORS5 in notice and AUTHORS5 in policy and AUTHORS5 in provenance and AUTHORS5 in erratum, "AUTHORS 5 hash")
require(ISC_BODY in notice and ISC_BODY in provenance and ISC_BODY in erratum, "LICENSE-ISC hash")
require("expression-only acceptance probe" in policy, "fixture probe wording")
require("separate pending" in policy, "OR-token pending")
require("does not weaken either rule" in policy, "no weakening")
require('gains one allow entry, `ISC`' in policy, "ISC allow statement")
require("`CDLA-Permissive-2.0` is not allowed" in policy, "CDLA still rejected")
require("Apache-2.0 AND ISC" in policy, "ring AND expression")
require("Selected licenses: Apache-2.0 and ISC. Both apply." in notice, "ring both-apply notice")
require("LICENSE-other-bits" in notice and "LICENSE-BoringSSL" in notice, "ring files")
require("That statement was wrong." in notice, "notice hides the initial error")
require(OLD_NOTICE in erratum, "erratum drops the old notice sentence")
require(OLD_POLICY_REFI in erratum, "erratum drops the old policy sentence")
require(OLD_POLICY_FIXTURE in erratum, "erratum drops the old fixture sentence")
require(OLD_NOTICE not in current, "current text still states the old notice sentence")
require(OLD_POLICY_REFI not in current, "current text still states the old MIT-absence sentence")
require(OLD_POLICY_FIXTURE not in current, "current text still calls the fixture the r-efi fact")
require("is not that license" not in current, "current text still says AUTHORS is not the license")

report = (A067 / "REPORT.md").read_text()
handoff = (A067 / "HANDOFF.md").read_text()
require(OLD_HANDOFF in handoff, "a067 handoff historical sentence missing")
require(OLD_REPORT_GAP in report, "a067 report gap sentence missing")
require(OLD_REPORT_COMMA in report, "a067 report comma gloss missing")
require("Its tree was not opened." in report, "a067 5.3.0 not-opened sentence missing")
require(OLD_HANDOFF in erratum and OLD_REPORT_GAP in erratum, "erratum does not cite the historical sentences")

fixture_license = (ISC / "fixtures/mit-or-gpl-missing-mit-text/LICENSE").read_text()
require(
    fixture_license == "This synthetic package declares MIT OR GPL-3.0-only and ships neither the MIT license text nor the GPL license text.\n",
    "fixture LICENSE semantics changed",
)
fixture_manifest = (ISC / "fixtures/mit-or-gpl-missing-mit-text/Cargo.toml").read_text()
require('license = "MIT OR GPL-3.0-only"' in fixture_manifest, "fixture expression")

deny = (ROOT / "ci/deny.toml").read_text()
require('"ISC"' in deny, "ISC allow removed")
require("CDLA-Permissive-2.0" not in deny, "CDLA added to deny.toml")

owned_forbidden = [
    "ci/deny.toml",
    "Cargo.toml",
    "Cargo.lock",
    "supply-chain/policy.md",
    "contracts/supply-chain/isc/fixtures",
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067",
]
dirty = git("status", "--porcelain", "--", *owned_forbidden)
require(dirty.strip() == "", f"owned-forbidden path dirty:\n{dirty}")
foreign = git(
    "status",
    "--porcelain",
    "--",
    "crates",
    "docs",
    "specs",
    "governance",
    "ci",
)
print("foreign_worktree_not_staged:")
print(foreign.rstrip() or "(none)")

pre = (
    ROOT
    / "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/pre-smoke/pre-edit-hashes.txt"
).read_text()
require(sha256(ROOT / "ci/deny.toml") in pre, "deny.toml hash drifted from pre-edit record")
require(sha256(ROOT / "Cargo.lock") in pre, "Cargo.lock hash drifted")
require(sha256(ROOT / "supply-chain/policy.md") in pre, "supply-chain/policy.md hash drifted")

if failures:
    print(f"FAILED {len(failures)}")
    for item in failures:
        print(f"- {item}")
    sys.exit(1)
print("ok")
