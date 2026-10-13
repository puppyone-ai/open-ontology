#!/usr/bin/env python3
"""Private TLS material and internal-user hashes. Secrets stay under /tmp."""

from __future__ import annotations

import os
import secrets
import stat
import string
import subprocess
import sys
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology/testing/dl01-driver-preflight/opensearch")
PRIVATE = Path("/tmp/oo-dl01w1-44-private")
IMAGE = "opensearchproject/opensearch@sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb"
NODE_DN = "CN=oo-dl01w1-44-node,OU=node,O=open-ontology"
ADMIN_CERT_DN = "CN=oo-dl01w1-44-dba-cert,OU=dba,O=open-ontology"


def strong_password() -> str:
    alphabet = string.ascii_letters + string.digits
    return "Aa1!" + "".join(secrets.choice(alphabet) for _ in range(32))


def run(cmd: list[str]) -> None:
    proc = subprocess.run(cmd, capture_output=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace")[-2000:])
        raise SystemExit(proc.returncode)


def openssl_cert(private: Path) -> None:
    certs = private / "certs"
    certs.mkdir(mode=0o700, exist_ok=True)
    (certs / "ca.cnf").write_text(
        "\n".join(
            [
                "[req]",
                "distinguished_name=dn",
                "x509_extensions=v3_ca",
                "prompt=no",
                "[dn]",
                "CN=oo-dl01w1-44-ca",
                "OU=DL01",
                "O=open-ontology",
                "[v3_ca]",
                "basicConstraints=critical,CA:TRUE",
                "keyUsage=critical,keyCertSign,cRLSign",
                "subjectKeyIdentifier=hash",
                "authorityKeyIdentifier=keyid:always,issuer",
                "",
            ]
        ),
        encoding="utf-8",
    )
    (certs / "node.cnf").write_text(
        "\n".join(
            [
                "[req]",
                "distinguished_name=dn",
                "x509_extensions=v3",
                "prompt=no",
                "[dn]",
                "CN=oo-dl01w1-44-node",
                "OU=node",
                "O=open-ontology",
                "[v3]",
                "subjectAltName=DNS:localhost,DNS:oo-dl01w1-44-opensearch,IP:127.0.0.1",
                "basicConstraints=CA:FALSE",
                "keyUsage=digitalSignature,keyEncipherment",
                "extendedKeyUsage=serverAuth",
                "",
            ]
        ),
        encoding="utf-8",
    )
    (certs / "admin.cnf").write_text(
        "\n".join(
            [
                "[req]",
                "distinguished_name=dn",
                "prompt=no",
                "[dn]",
                "CN=oo-dl01w1-44-dba-cert",
                "OU=dba",
                "O=open-ontology",
                "[v3]",
                "basicConstraints=CA:FALSE",
                "keyUsage=digitalSignature,keyEncipherment",
                "extendedKeyUsage=clientAuth",
                "",
            ]
        ),
        encoding="utf-8",
    )
    run(["openssl", "req", "-x509", "-newkey", "rsa:3072", "-sha256", "-days", "30", "-nodes",
         "-keyout", str(certs / "ca.key"), "-out", str(certs / "ca.pem"),
         "-config", str(certs / "ca.cnf"), "-extensions", "v3_ca"])
    run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(certs / "node.key"),
         "-out", str(certs / "node.csr"), "-config", str(certs / "node.cnf")])
    run(["openssl", "x509", "-req", "-in", str(certs / "node.csr"), "-CA", str(certs / "ca.pem"),
         "-CAkey", str(certs / "ca.key"), "-CAcreateserial", "-out", str(certs / "node.pem"),
         "-days", "30", "-sha256", "-extensions", "v3", "-extfile", str(certs / "node.cnf")])
    run(["openssl", "pkcs8", "-topk8", "-nocrypt", "-in", str(certs / "node.key"), "-out", str(certs / "node-key.pem")])
    run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(certs / "admin.key"),
         "-out", str(certs / "admin.csr"), "-config", str(certs / "admin.cnf")])
    run(["openssl", "x509", "-req", "-in", str(certs / "admin.csr"), "-CA", str(certs / "ca.pem"),
         "-CAkey", str(certs / "ca.key"), "-CAcreateserial", "-out", str(certs / "admin.pem"),
         "-days", "30", "-sha256", "-extensions", "v3", "-extfile", str(certs / "admin.cnf")])
    run(["openssl", "pkcs8", "-topk8", "-nocrypt", "-in", str(certs / "admin.key"), "-out", str(certs / "admin-key.pem")])
    for name in ("ca.key", "node.key", "node-key.pem", "admin.key", "admin-key.pem"):
        os.chmod(certs / name, 0o600)
    node_dn = subprocess.check_output(
        ["openssl", "x509", "-in", str(certs / "node.pem"), "-noout", "-subject", "-nameopt", "RFC2253"],
        text=True,
    ).strip()
    admin_dn = subprocess.check_output(
        ["openssl", "x509", "-in", str(certs / "admin.pem"), "-noout", "-subject", "-nameopt", "RFC2253"],
        text=True,
    ).strip()
    (private / "dn.txt").write_text(
        f"node={node_dn}\nadmin_cert={admin_dn}\nexpected_node={NODE_DN}\nexpected_admin={ADMIN_CERT_DN}\n",
        encoding="utf-8",
    )

    def rdns(value: str) -> set[str]:
        return {part for part in value.replace("subject=", "").replace(" ", "").split(",") if part}

    if rdns(node_dn) != rdns(NODE_DN) or rdns(admin_dn) != rdns(ADMIN_CERT_DN):
        sys.stderr.write("certificate DN attributes did not match the pinned subject\n")
        raise SystemExit(2)


def hash_password(password: str) -> str:
    proc = subprocess.run(
        [
            "docker", "run", "--rm", "-i",
            "--memory", "512m", "--memory-swap", "512m",
            "--name", "oo-dl01w1-44-hash",
            "--label", "project=open-ontology",
            "--label", "owner=oo-dl01w1-44",
            "--label", "session=bc6cf794-2d14-4e98-bcb6-f7a9b3533a88",
            "--label", "kind=target-preparation",
            "--entrypoint", "bash",
            IMAGE,
            "-lc",
            "PW=$(cat); /usr/share/opensearch/plugins/opensearch-security/tools/hash.sh -p \"$PW\"",
        ],
        input=password.encode("utf-8") + b"\n",
        capture_output=True,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace")[-2000:])
        raise SystemExit(proc.returncode)
    hashed = next((line.strip() for line in proc.stdout.decode().splitlines() if line.startswith("$2")), "")
    if not hashed.startswith("$2"):
        sys.stderr.write("hash tool did not return a bcrypt hash\n")
        raise SystemExit(2)
    return hashed


def write_users(private: Path, dba_password: str, reader_password: str, dba_hash: str, reader_hash: str) -> None:
    sec = private / "security"
    sec.mkdir(mode=0o700, exist_ok=True)
    src = ROOT / "config" / "security"
    for name in (
        "config.yml", "roles.yml", "roles_mapping.yml", "action_groups.yml",
        "tenants.yml", "allowlist.yml", "audit.yml", "nodes_dn.yml",
    ):
        target = sec / name
        target.write_text((src / name).read_text(encoding="utf-8"), encoding="utf-8")
        os.chmod(target, stat.S_IRUSR | stat.S_IWUSR)
    (sec / "internal_users.yml").write_text(
        "\n".join(
            [
                "---",
                "_meta:",
                '  type: "internalusers"',
                "  config_version: 2",
                "",
                "oo_dl01w1_44_dba:",
                f'  hash: "{dba_hash}"',
                "  reserved: false",
                '  description: "setup identity"',
                "",
                "oo_dl01w1_44_reader:",
                f'  hash: "{reader_hash}"',
                "  reserved: false",
                '  description: "synthetic index reader"',
                "",
            ]
        ),
        encoding="utf-8",
    )
    (private / "identities.env").write_text(
        "\n".join(
            [
                "dba_user=oo_dl01w1_44_dba",
                f"dba_password={dba_password}",
                "reader_user=oo_dl01w1_44_reader",
                f"reader_password={reader_password}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    os.chmod(private / "identities.env", stat.S_IRUSR | stat.S_IWUSR)


def main() -> None:
    if PRIVATE.exists():
        sys.stderr.write("private directory already exists; refusing to reuse secrets\n")
        raise SystemExit(2)
    PRIVATE.mkdir(mode=0o700)
    openssl_cert(PRIVATE)
    dba_password = strong_password()
    reader_password = strong_password()
    dba_hash = hash_password(dba_password)
    reader_hash = hash_password(reader_password)
    write_users(PRIVATE, dba_password, reader_password, dba_hash, reader_hash)
    (PRIVATE / "opensearch.yml").write_text((ROOT / "config" / "opensearch.yml").read_text(encoding="utf-8"), encoding="utf-8")
    (PRIVATE / "endpoint.url").write_text("https://oo-dl01w1-44-opensearch:9200\n", encoding="utf-8")
    # Parent traversal protects mounted TLS/security files on the host.
    # Raw identity credentials are never mounted into the database container.
    for path in PRIVATE.rglob("*"):
        os.chmod(path, 0o755 if path.is_dir() else 0o644)
    os.chmod(PRIVATE / "identities.env", 0o600)
    os.chmod(PRIVATE, 0o700)
    print("private_dir_mode=0700")
    print("identities=oo_dl01w1_44_dba,oo_dl01w1_44_reader")
    print("secrets_written=1")


if __name__ == "__main__":
    main()
