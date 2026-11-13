#!/usr/bin/env python3
"""Offline source-license inventory, not legal clearance or a linked-artifact SBOM.

Usage: python3 licenses.py --output /absolute/outside-git/new-directory
Requires installed Cargo/Rust and their existing registry cache; never downloads.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tomllib


TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
NOTICE_NAME = re.compile(r"^(?:licen[cs]e|copying|notice|copyright)", re.I)
LOCAL_PATH = re.compile(r"(?<![A-Za-z0-9:/])(?:/[^\s)]+|[A-Za-z]:[\\/][^\s)]+)")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def public_text(value):
    return LOCAL_PATH.sub("[local-path-redacted]", value) if isinstance(value, str) else None


def command(args):
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=120,
                                env={**os.environ, "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never"})
    except (OSError, subprocess.TimeoutExpired):
        raise RuntimeError(f"{args[0]} unavailable or timed out") from None
    if result.returncode:
        # Cargo diagnostics can contain private registry URLs and absolute paths.
        raise RuntimeError(f"{args[0]} {args[1]} failed offline; inspect local tools/cache separately") from None
    return result.stdout.strip()


def select_packages(metadata, trees, package_name="oo-cli"):
    """Cargo tree supplies the package-specific normal/build closure, not workspace metadata features."""
    by_version = {}
    for package in metadata["packages"]:
        by_version.setdefault((package["name"], package["version"]), []).append(package)
    selected = {}
    for target, tree in sorted(trees.items()):
        for line in tree.splitlines():
            if not line.strip():
                continue
            match = re.match(r"^([A-Za-z0-9_-]+) v([A-Za-z0-9.+-]+)(?:\s|$)", line)
            if not match:
                raise RuntimeError("Unrecognized Cargo tree entry; closure is not verified")
            candidates = by_version.get(match.groups(), [])
            if len(candidates) != 1:
                raise RuntimeError("Ambiguous or missing Cargo package identity; closure is not verified")
            package = candidates[0]
            item = selected.setdefault(package["id"], {"package": package, "targets": set()})
            item["targets"].add(target)
    if not any(item["package"]["name"] == package_name for item in selected.values()):
        raise RuntimeError(f"{package_name} is missing from the dependency closure")
    return sorted(selected.values(), key=lambda item: (
        item["package"]["name"], item["package"]["version"]))


def source_label(package, workspace_members):
    source = package.get("source")
    if source in ("registry+https://github.com/rust-lang/crates.io-index",
                  "sparse+https://index.crates.io/"):
        return "crates.io"
    if not source:
        return "workspace" if package["id"] in workspace_members else "local-path"
    return "git (URL omitted)" if source.startswith("git+") else "registry (URL omitted)"


def notice_paths(root):
    for directory, names, files in os.walk(root, followlinks=False):
        names[:] = sorted(name for name in names if name not in {".git", "target", "node_modules"})
        folder = Path(directory)
        in_licenses = any(part.lower() in {"licenses", "licences"} for part in folder.relative_to(root).parts)
        for name in sorted(files):
            if NOTICE_NAME.match(name) or in_licenses:
                yield folder / name


def read_notices(candidates):
    files, texts, gaps = [], [], []
    for label, path, allowed_root in sorted(candidates, key=lambda item: item[0]):
        label = public_text(label)
        try:
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(allowed_root.resolve()) or not resolved.is_file():
                gaps.append(f"notice_outside_source:{label}")
                continue
            raw = resolved.read_bytes()
            body = raw.decode("utf-8")
            if not body.strip():
                gaps.append(f"empty_notice:{label}")
                continue
        except (OSError, UnicodeError):
            gaps.append(f"notice_missing_or_unreadable:{label}")
            continue
        files.append({"path": label, "sha256": sha256(raw)})
        texts.append(f"--- {label} | SHA256 {sha256(raw)} ---\n{body.rstrip()}\n")
    text = "\n".join(texts)
    return files, text, sorted(set(gaps))


def load_supplements(directory=None):
    directory = directory or Path(__file__).resolve().parents[1] / "licenses"
    manifest = directory / "manifest.json"
    if not manifest.exists():
        return {}
    try:
        data = json.loads(manifest.read_text())
        if data.get("schema_version") != 1 or data.get("legal_clearance") is not False:
            raise ValueError()
        result = {}
        for entry in data["supplements"]:
            required = ("name", "version", "repository", "vcs_commit", "path_in_vcs",
                        "license", "file", "upstream_path", "upstream_url", "sha256")
            if any(not isinstance(entry.get(key), str) or (not entry[key] and key != "path_in_vcs")
                   for key in required):
                raise ValueError()
            if (entry.get("source") != "crates.io"
                    or not re.fullmatch(r"[A-Za-z0-9_-]+", entry["name"])
                    or not re.fullmatch(r"[A-Za-z0-9.+-]+", entry["version"])
                    or not re.fullmatch(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+",
                                        entry["repository"])
                    or not re.fullmatch(r"[a-f0-9]{40}", entry["vcs_commit"])
                    or not re.fullmatch(r"[a-f0-9]{64}", entry["sha256"])
                    or public_text(entry["license"]) != entry["license"]):
                raise ValueError()
            for key in ("path_in_vcs",):
                if not entry[key]:
                    continue
                path = PurePosixPath(entry[key])
                if (path.is_absolute() or ".." in path.parts or path.as_posix() != entry[key]
                        or not re.fullmatch(r"[A-Za-z0-9_./-]+", entry[key])):
                    raise ValueError()
            prefix = (entry["repository"].replace("https://github.com/", "https://raw.githubusercontent.com/")
                      + "/" + entry["vcs_commit"] + "/")
            additional = entry.get("additional_files", [])
            if not isinstance(additional, list):
                raise ValueError()
            seen = set()
            for notice in [entry, *additional]:
                for key in ("file", "upstream_path"):
                    path = PurePosixPath(notice[key])
                    if (path.is_absolute() or ".." in path.parts or path.as_posix() != notice[key]
                            or not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_./-]*", notice[key])):
                        raise ValueError()
                if (notice["file"] in seen
                        or not re.fullmatch(r"[a-f0-9]{64}", notice["sha256"])
                        or notice["upstream_url"] != prefix + notice["upstream_path"]):
                    raise ValueError()
                seen.add(notice["file"])
            provenance = entry["provenance"]
            expected = {"crate_manifest_url", "crate_manifest_sha256", "registry_vcs_info_sha256"}
            if "vcs_dirty" in entry:
                if entry["vcs_dirty"] is not True:
                    raise ValueError()
                expected.add("registry_manifest_sha256")
            if "workspace_manifest_url" in provenance or "workspace_manifest_sha256" in provenance:
                expected.update({"workspace_manifest_url", "workspace_manifest_sha256"})
            if (set(provenance) != expected
                    or provenance["crate_manifest_url"] != prefix + (
                        entry["path_in_vcs"] + "/" if entry["path_in_vcs"] else "") + "Cargo.toml"
                    or ("workspace_manifest_url" in provenance and provenance["workspace_manifest_url"] != prefix + "Cargo.toml")
                    or any(not isinstance(value, str) or not re.fullmatch(r"[a-f0-9]{64}", value)
                           for key, value in provenance.items() if key.endswith("_sha256"))):
                raise ValueError()
            key = (entry["name"], entry["version"])
            if key in result:
                raise ValueError()
            result[key] = (entry, directory)
        return result
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        raise RuntimeError("Pinned license supplement manifest is invalid") from None


def supplemental_notices(package, root, members, supplements):
    match = supplements.get((package["name"], package["version"]))
    if not match:
        return [], "", []
    entry, directory = match
    if (source_label(package, members) != entry["source"]
            or (package.get("repository") or "").rstrip("/") != entry["repository"]
            or package.get("license") != entry["license"]):
        return [], "", ["license_supplement_package_mismatch"]
    try:
        vcs_raw = (root / ".cargo_vcs_info.json").read_bytes()
        vcs = json.loads(vcs_raw)
        if (vcs["git"]["sha1"] != entry["vcs_commit"]
                or vcs["git"].get("dirty", False) is not entry.get("vcs_dirty", False)
                or vcs["path_in_vcs"] != entry["path_in_vcs"]
                or sha256(vcs_raw) != entry["provenance"]["registry_vcs_info_sha256"]):
            return [], "", ["license_supplement_vcs_mismatch"]
        # A published dirty VCS record also pins the packaged manifest, never a blanket bypass.
        if entry.get("vcs_dirty") and sha256((root / "Cargo.toml.orig").read_bytes()) != entry["provenance"]["registry_manifest_sha256"]:
            return [], "", ["license_supplement_package_mismatch"]
    except (OSError, ValueError, KeyError, TypeError):
        return [], "", ["license_supplement_vcs_unavailable"]
    files, sections = [], []
    for notice in [entry, *entry.get("additional_files", [])]:
        found, body, gaps = read_notices([
            ("supplements/" + notice["file"], directory / notice["file"], directory)])
        if not found or gaps:
            return [], "", gaps or ["license_supplement_text_missing"]
        if found[0]["sha256"] != notice["sha256"]:
            return [], "", ["license_supplement_digest_mismatch"]
        found[0]["provenance"] = {
            "kind": "pinned_upstream_supplement", "repository": entry["repository"],
            "vcs_commit": entry["vcs_commit"], "path_in_vcs": entry["path_in_vcs"],
            "upstream_url": notice["upstream_url"], **entry["provenance"],
        }
        files.extend(found)
        sections.append(f"Pinned upstream: {notice['upstream_url']}\nVCS commit: {entry['vcs_commit']}\n{body}")
    return files, "\n".join(sections), []


def package_inventory(package, targets, workspace, members, supplements=None):
    root = Path(package["manifest_path"]).parent
    candidates = {path.relative_to(root).as_posix(): (path, root) for path in notice_paths(root)}
    gaps = []
    if package.get("license_file"):
        path = root / package["license_file"]
        if path.resolve().is_relative_to(root.resolve()):
            candidates[path.resolve().relative_to(root.resolve()).as_posix()] = (path, root)
        else:
            gaps.append("declared_license_file_outside_package")
    # Only first-party workspace members that explicitly inherit the workspace license use its notices.
    if package["id"] in members:
        manifest = tomllib.loads(Path(package["manifest_path"]).read_text())
        if manifest.get("package", {}).get("license") == {"workspace": True}:
            for name in ("LICENSE", "NOTICE"):
                candidates[f"workspace/{name}"] = (workspace / name, workspace)
    files, body, file_gaps = read_notices([(label, path, base) for label, (path, base) in candidates.items()])
    if not files and supplements:
        files, body, supplement_gaps = supplemental_notices(package, root, members, supplements)
        file_gaps.extend(supplement_gaps)
    license_expression = public_text(package.get("license"))
    if not license_expression:
        gaps.append("license_expression_missing")
    elif license_expression != package.get("license"):
        gaps.append("license_expression_local_path_redacted")
    if not files:
        gaps.append("license_text_missing")
    if package["name"] == "aws-lc-sys" and not any(item["path"].startswith("aws-lc/") for item in files):
        gaps.append("aws_lc_vendored_notice_missing")
    record = {
        "name": package["name"], "version": package["version"],
        "source": source_label(package, members), "license": license_expression,
        "targets": sorted(targets), "license_files": files,
        "text_sha256": sha256(body.encode()) if body else None,
        "unresolved": sorted(set(gaps + file_gaps)),
    }
    return record, body


def toolchain_inventory(targets):
    try:
        sysroot = Path(command(["rustc", "--print", "sysroot"]))
        version = public_text(command(["rustc", "--version"]))
        verbose = command(["rustc", "--version", "--verbose"])
    except RuntimeError:
        return {"version": None, "license_files": [], "text_sha256": None,
                "unresolved": ["rust_toolchain_unavailable"]}, ""
    paths = ("LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT",
             "share/doc/rustc/COPYRIGHT-library.html")
    files, body, gaps = read_notices([(name, sysroot / name, sysroot) for name in paths])
    present = {item["path"] for item in files}
    if "share/doc/rustc/COPYRIGHT-library.html" not in present:
        gaps.append("rust_standard_library_third_party_notice_missing")
    host = next((line[6:] for line in verbose.splitlines() if line.startswith("host: ")), None)
    return {
        "version": version, "host": public_text(host),
        "scope": "Installed toolchain root licenses and standard-library source notices; not proof of artifact linkage.",
        "target_std_installed": {target: (sysroot / "lib/rustlib" / target / "lib").is_dir()
                                 for target in sorted(targets)},
        "license_files": files, "text_sha256": sha256(body.encode()) if body else None,
        "unresolved": sorted(set(gaps)),
    }, body


def outside_git(path):
    resolved = path.expanduser().resolve()
    for parent in (resolved, *resolved.parents):
        if (parent / ".git").exists():
            top = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=parent,
                                 capture_output=True, text=True)
            if (top.returncode or top.stdout.strip() != str(parent)
                    or subprocess.run(["git", "check-ignore", "--quiet", str(resolved)], cwd=parent,
                                      capture_output=True).returncode):
                raise RuntimeError("Output must be outside Git repositories or git-ignored")
            break
    if resolved.exists():
        raise RuntimeError("Output directory must not already exist")
    return resolved


def generate(manifest, output, targets):
    output = outside_git(output)
    args = ["--offline", "--locked", "--manifest-path", str(manifest.resolve())]
    metadata = json.loads(command(["cargo", "metadata", "--format-version", "1", *args]))
    trees, unresolved = {}, []
    for target in sorted(set(targets)):
        try:
            trees[target] = command(["cargo", "tree", *args, "-p", "oo-cli", "--target", target,
                                     "--edges", "normal,build", "--prefix", "none", "--format", "{p}"])
        except RuntimeError:
            unresolved.append({"component": target, "fields": ["normal_build_closure_unavailable_offline"]})
    selected = select_packages(metadata, trees) if trees else []
    workspace = Path(metadata["workspace_root"])
    members = set(metadata["workspace_members"])
    # The stdio bridge has its own lockfile; include its non-live dependency
    # closure rather than treating the CLI workspace as a complete inventory.
    helper_locks = {}
    for helper, relative in (("mcp-gateway", "tools/mcp-gateway/Cargo.toml"),
                             ("oo-mysql-doctor", "deploy/lite/mysql-doctor/Cargo.toml")):
        helper_manifest = workspace / relative
        helper_args = ["--offline", "--locked", "--manifest-path", str(helper_manifest)]
        helper_metadata = json.loads(command(["cargo", "metadata", "--format-version", "1", *helper_args]))
        helper_trees = {target: command(["cargo", "tree", *helper_args, "-p", helper,
            "--target", target, "--edges", "normal,build", "--prefix", "none", "--format", "{p}"])
            for target in sorted(set(targets))}
        combined = {item["package"]["id"]: item for item in selected}
        for item in select_packages(helper_metadata, helper_trees, helper):
            existing = combined.setdefault(item["package"]["id"], item)
            existing["targets"].update(item["targets"])
        selected = sorted(combined.values(), key=lambda item: (item["package"]["name"], item["package"]["version"]))
        members.update(helper_metadata["workspace_members"])
        helper_locks[helper] = sha256(helper_manifest.with_name("Cargo.lock").read_bytes())
    supplements = load_supplements()
    dependencies, sections = [], []
    for item in selected:
        record, body = package_inventory(item["package"], item["targets"], workspace, members, supplements)
        dependencies.append(record)
        heading = f"{record['name']} {record['version']} | {record['source']}\nDeclared license: {record['license'] or 'UNRESOLVED'}"
        if record["unresolved"]:
            unresolved.append({"component": f"{record['name']}@{record['version']}",
                               "fields": record["unresolved"]})
        sections.append(f"=== {heading} ===\nUnresolved: {', '.join(record['unresolved']) or 'none detected by file collection'}\n\n{body}")
    toolchain, body = toolchain_inventory(targets)
    if toolchain["unresolved"]:
        unresolved.append({"component": "rust-toolchain", "fields": toolchain["unresolved"]})
    sections.append(f"=== Rust toolchain and standard-library notices ===\n{body}")
    unresolved.append({"component": "artifact", "fields": [
        "binary_digest_and_linkage_not_verified",
        "per_target_build_toolchain_not_verified",
        "system_dynamic_libraries_not_in_cargo_inventory",
        "license_obligations_and_vendored_notice_completeness_not_legally_reviewed",
    ]})
    inventory = {
        "schema_version": 1, "package": "oo-cli", "legal_clearance": False,
        "scope": "Cargo-selected normal+build dependency source notices; excludes dev-only and unrelated workspace packages.",
        "targets_requested": sorted(set(targets)), "targets_resolved": sorted(trees),
        "cargo_version": public_text(command(["cargo", "--version"])),
        "cargo_lock_sha256": sha256((workspace / "Cargo.lock").read_bytes()),
        "mcp_cargo_lock_sha256": helper_locks["mcp-gateway"],
        "mysql_doctor_cargo_lock_sha256": helper_locks["oo-mysql-doctor"],
        "notice_selection": "Source notice files and declared license_file; absent texts may use offline supplements pinned to exact crate version, repository, VCS commit/path and SHA256.",
        "text_sha256_definition": "SHA256 of each component's UTF-8 rendered notice block; license_files hashes cover original bytes.",
        "dependencies": dependencies, "rust_toolchain": toolchain,
        "unresolved": sorted(unresolved, key=lambda item: item["component"]),
    }
    intro = (
        "Open Ontology native CLI - collected source license texts\n"
        "NOT LEGAL CLEARANCE. NOT A VERIFIED PER-BINARY SBOM.\n"
        "Normal/build dependency sources for: " + ", ".join(inventory["targets_resolved"]) + "\n"
        "Source notice collection may include vendored files not linked into a binary.\n"
        "Pinned upstream supplements are identified with commit URLs and hashes; generation never downloads.\n"
        "Declared expressions are not license selections or an obligations review.\n"
        "See dependencies.json for hashes, target membership and all unresolved fields.\n\n"
        "UNRESOLVED:\n" + "\n".join(f"- {item['component']}: {', '.join(item['fields'])}"
                                     for item in inventory["unresolved"]) + "\n\n"
    )
    output.mkdir(parents=True, mode=0o700)
    (output / "THIRD_PARTY_NOTICES.txt").write_text(intro + "\n".join(sections), encoding="utf-8")
    (output / "dependencies.json").write_text(json.dumps(inventory, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
                                             encoding="utf-8")
    return inventory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest-path", type=Path, default=Path(__file__).resolve().parents[3] / "Cargo.toml")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", action="append", choices=TARGETS)
    args = parser.parse_args()
    inventory = generate(args.manifest_path, args.output, args.target or TARGETS)
    print(json.dumps({"dependencies": len(inventory["dependencies"]),
                      "targets_resolved": inventory["targets_resolved"],
                      "unresolved": inventory["unresolved"], "legal_clearance": False}, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        message = str(error) if isinstance(error, RuntimeError) else "Inventory failed; check local metadata and source files"
        print(f"license inventory: {public_text(message)}", file=sys.stderr)
        sys.exit(1)
