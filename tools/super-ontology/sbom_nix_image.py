#!/usr/bin/env python3
"""CycloneDX-subset SBOM of a nix-built Postgres image (the pgvector admission image).

    sbom_nix_image.py IMAGE [--label oo.lane=...] > sbom.json
    python3 tools/doc-parser/sbom_gate.py sbom.json [--reviews r.json]

Reads the runtime closure of the postgres-and-plugins store path (nix-store -qR) and the
dpkg database from inside the image. Nix store paths carry no licence metadata, so the
licence of each component is the upstream licence by package name from the table below
(maintainer assertion, not a file scan); a package missing from the table is NOASSERTION
and the gate reports it as unknown. dpkg packages are scope base-os (listed, not gated).
"""
import json
import re
import subprocess
import sys

LICENSES = [
    (r"^(abseil-cpp|protobuf|sfcgal-.*)$", None),  # decided below
    (r"^abseil-cpp$", "Apache-2.0"), (r"^protobuf$", "BSD-3-Clause"),
    (r"^(acl|attr|audit|cracklib|gnutls|libgcrypt|libgpg-error|libassuan|libidn2|libnl|libmnl|libnftnl|libnfnetlink|libnetfilter_conntrack|libcap|libapparmor|libseccomp|libpwquality|keyutils|libkrb5-dev|lvm2|libunistring|libxcrypt|libmicrohttpd|geos|libossp-uuid|libpcap)$", None),
    (r"^bash(-interactive)?$", "GPL-3.0-or-later"),
    (r"^(coreutils|gnugrep|gnum4|gnutar|gzip|gnupg|kbd|kexec-tools|cryptsetup|bison|npth)$", None),
    (r"^boost$", "BSL-1.0"), (r"^brotli$", "MIT"), (r"^bzip2$", "bzip2-1.0.6"),
    (r"^curl$", "curl"), (r"^expat$", "MIT"), (r"^gtest$", "BSD-3-Clause"),
    (r"^gdal$", "MIT"), (r"^proj$", "MIT"), (r"^libgeotiff$", "MIT"), (r"^libtiff$", "libtiff"),
    (r"^lerc$", "Apache-2.0"), (r"^libdeflate$", "MIT"), (r"^libjpeg-turbo$", "BSD-3-Clause AND IJG"),
    (r"^libffi$", "MIT"), (r"^json-c$", "MIT"), (r"^libevent$", "BSD-3-Clause"), (r"^libsodium$", "ISC"),
    (r"^libssh2$", "BSD-3-Clause"), (r"^libtasn1$", "LGPL-2.1-or-later"), (r"^libpsl$", "MIT"),
    (r"^libxml2$", "MIT"), (r"^libxslt$", "MIT"), (r"^lz4$", "BSD-2-Clause AND GPL-2.0-or-later"),
    (r"^nettle$", "LGPL-3.0-or-later OR GPL-2.0-or-later"), (r"^gmp-with-cxx$", "LGPL-3.0-or-later OR GPL-2.0-or-later"),
    (r"^mpfr$", "LGPL-3.0-or-later"), (r"^ncurses$", "X11"), (r"^openssl$", "Apache-2.0"),
    (r"^pcre2$", "BSD-3-Clause"), (r"^readline$", "GPL-3.0-or-later"), (r"^sqlite$", "blessing"),
    (r"^xz$", "0BSD AND GPL-2.0-or-later"), (r"^zlib$", "Zlib"), (r"^zstd$", "BSD-3-Clause OR GPL-2.0-only"),
    (r"^xxHash$", "BSD-2-Clause AND GPL-2.0-or-later"), (r"^icu4c$", "Unicode-DFS-2016"),
    (r"^(glibc|getent-glibc)$", "LGPL-2.1-or-later"), (r"^glibc-bin$", "LGPL-2.1-or-later"),
    (r"^(gcc|xgcc)$", "GPL-3.0-or-later WITH GCC-exception-3.1"),
    (r"^systemd(-minimal-libs)?$", "LGPL-2.1-or-later"), (r"^kmod$", "LGPL-2.1-or-later"),
    (r"^libbpf$", "LGPL-2.1-only OR BSD-2-Clause"), (r"^elfutils$", "GPL-2.0-or-later OR LGPL-3.0-or-later"),
    (r"^linux-pam$", "BSD-3-Clause"), (r"^iptables$", "GPL-2.0-or-later"),
    (r"^util-linux-minimal$", "GPL-2.0-or-later"), (r"^perl$", "Artistic-1.0-Perl OR GPL-1.0-or-later"),
    (r"^flex$", "BSD-2-Clause"), (r"^tzdata$", "LicenseRef-tzdata-public-domain"),
    (r"^libfido2$", "BSD-2-Clause"), (r"^libcbor$", "MIT"), (r"^libargon2$", "CC0-1.0 OR Apache-2.0"),
    (r"^unbound$", "BSD-3-Clause"), (r"^pcsclite$", "BSD-3-Clause"), (r"^p11-kit$", "BSD-3-Clause"),
    (r"^tpm2-tss$", "BSD-2-Clause"), (r"^qrencode$", "LGPL-2.1-or-later"), (r"^msgpack-c$", "BSL-1.0"),
    (r"^db$", "Sleepycat"), (r"^libkrb5$", "MIT"), (r"^lvm2$", "GPL-2.0-only AND LGPL-2.1-only"),
    (r"^(vector)$", "PostgreSQL"), (r"^(postgresql|postgresql-and-plugins)$", "PostgreSQL"),
    (r"^(hypopg|pgaudit|pg_cron|pgjwt|pg_partman|pg_repack|pgtap|plpgsql_check|pg_hashids|pgmq|pg_stat_monitor|rum|pg_tle|wal2json|pgroonga)$", None),
    (r"^(wrappers|pg_graphql|pg_jsonschema|pg_net|supabase_vault|pgsodium|supautils|plan_filter|safeupdate|index_advisor|switch-ext-version|http|pgrouting|postgis|mecab.*|kytea|supabase-groonga|sfcgal|libgeos|dns-root-data|publicsuffix-list|find-xml-catalogs-hook|overlayfs-on-package|libossp-uuid|protobuf-c)$", None),
]
# Upstream licences by name for entries above marked None, plus the rest asserted individually.
ASSERT = {
    "acl": "LGPL-2.1-or-later AND GPL-2.0-or-later", "attr": "LGPL-2.1-or-later AND GPL-2.0-or-later",
    "audit": "LGPL-2.1-or-later AND GPL-2.0-or-later", "cracklib": "LGPL-2.1-or-later",
    "gnutls": "LGPL-2.1-or-later", "libgcrypt": "LGPL-2.1-or-later", "libgpg-error": "LGPL-2.1-or-later",
    "libassuan": "LGPL-2.1-or-later", "libidn2": "GPL-2.0-or-later OR LGPL-3.0-or-later",
    "libnl": "LGPL-2.1-only", "libmnl": "LGPL-2.1-or-later", "libnftnl": "GPL-2.0-or-later",
    "libnfnetlink": "GPL-2.0-or-later", "libnetfilter_conntrack": "GPL-2.0-or-later",
    "libcap": "BSD-3-Clause OR GPL-2.0-only", "libapparmor": "LGPL-2.1-or-later",
    "libseccomp": "LGPL-2.1-only", "libpwquality": "BSD-3-Clause OR GPL-2.0-or-later",
    "keyutils": "GPL-2.0-or-later AND LGPL-2.1-or-later", "libunistring": "GPL-3.0-or-later OR LGPL-3.0-or-later",
    "libxcrypt": "LGPL-2.1-or-later", "libmicrohttpd": "LGPL-2.1-or-later", "geos": "LGPL-2.1-or-later",
    "libpcap": "BSD-3-Clause", "libossp-uuid": "MIT",
    "coreutils": "GPL-3.0-or-later", "gnugrep": "GPL-3.0-or-later", "gnum4": "GPL-3.0-or-later",
    "gnutar": "GPL-3.0-or-later", "gzip": "GPL-3.0-or-later", "gnupg": "GPL-3.0-or-later",
    "kbd": "GPL-2.0-or-later", "kexec-tools": "GPL-2.0-only", "cryptsetup": "GPL-2.0-or-later",
    "bison": "GPL-3.0-or-later", "npth": "LGPL-2.1-or-later",
    "hypopg": "PostgreSQL", "pgaudit": "PostgreSQL", "pg_cron": "PostgreSQL", "pgjwt": "MIT",
    "pg_partman": "PostgreSQL", "pg_repack": "BSD-3-Clause", "pgtap": "PostgreSQL",
    "plpgsql_check": "PostgreSQL", "pg_hashids": "MIT", "pgmq": "PostgreSQL",
    "pg_stat_monitor": "PostgreSQL", "rum": "PostgreSQL", "pg_tle": "Apache-2.0",
    "wal2json": "BSD-3-Clause", "pgroonga": "PostgreSQL",
    "wrappers": "Apache-2.0", "pg_graphql": "Apache-2.0", "pg_jsonschema": "Apache-2.0",
    "pg_net": "Apache-2.0", "supabase_vault": "Apache-2.0", "pgsodium": "BSD-3-Clause",
    "supautils": "Apache-2.0", "plan_filter": "PostgreSQL", "safeupdate": "MIT",
    "pgrouting": "GPL-2.0-or-later", "postgis": "GPL-2.0-or-later",
    "mecab": "GPL-2.0-only OR LGPL-2.1-only OR BSD-3-Clause", "kytea": "LGPL-2.1-or-later",
    "supabase-groonga": "LGPL-2.1-only", "sfcgal": "LGPL-2.0-or-later",
    "protobuf-c": "BSD-2-Clause", "find-xml-catalogs-hook": "MIT",
    # no confident upstream licence: left unasserted on purpose
}


def lic(name):
    if name in ASSERT:
        return ASSERT[name]
    for pat, expr in LICENSES:
        if expr and re.match(pat, name):
            return expr
    return None


def sh(image, label, script):
    cmd = ["docker", "run", "--rm", "--label", label, "--entrypoint", "sh", image, "-c", script]
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def main():
    image = sys.argv[1]
    label = sys.argv[sys.argv.index("--label") + 1] if "--label" in sys.argv else "oo.lane=super-ontology-c8"
    out = sh(image, label, 'for p in /nix/store/*postgresql-and-plugins-*; do case "$p" in *.drv) ;; *) nix-store -qR "$p";; esac; done | sed "s|/nix/store/[a-z0-9]*-||" | sort -u; echo @@DPKG; dpkg-query -W -f="\\${Package}\\t\\${Version}\\n"')
    nix, _, dpkg = out.partition("@@DPKG\n")
    comps, seen = [], set()
    for line in nix.split():
        m = re.match(r"^(.*?)-(\d[^-]*(?:-[0-9a-f]{32,})?.*?)$", line)
        name, ver = (m.group(1), m.group(2)) if m else (line, "")
        name = re.sub(r"-(lib|bin|dev|man|getent)$", "", name)
        key = (name, ver)
        if key in seen or name in ("glibc-bin",) and ("glibc", ver) in seen:
            continue
        seen.add(key)
        expr = lic(name)
        c = {"type": "library", "name": name, "version": ver, "licenses": [{"expression": expr}] if expr else [],
             "properties": [{"name": "oo:scope", "value": "native-in-image"}, {"name": "oo:licence_source", "value": "upstream licence by name (maintainer assertion)" if expr else "none"}]}
        comps.append(c)
    for line in dpkg.splitlines():
        n, _, v = line.partition("\t")
        comps.append({"type": "library", "name": n, "version": v, "licenses": [],
                      "properties": [{"name": "oo:scope", "value": "base-os"}]})
    print(json.dumps({"bomFormat": "CycloneDX", "specVersion": "1.5", "metadata": {"component": {"name": image}}, "components": comps}, indent=1))


if __name__ == "__main__":
    main()
