import copy
import json
import subprocess
from pathlib import Path
import tempfile
import unittest

from licenses import TARGETS, load_supplements, outside_git, package_inventory, select_packages, sha256


class LicenseInventoryTests(unittest.TestCase):
    def test_ignored_artifacts_allowed_but_tracked_source_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            (root / ".gitignore").write_text("/evidence/\n")
            self.assertEqual(outside_git(root / "evidence/notices"), root / "evidence/notices")
            with self.assertRaisesRegex(RuntimeError, "outside Git"):
                outside_git(root / "src/notices")

    def supplement_fixture(self, root):
        crate, directory = root / "crate", root / "supplements"
        crate.mkdir()
        directory.mkdir()
        commit = "a" * 40
        prefix = f"https://raw.githubusercontent.com/example/project/{commit}/"
        vcs = {"git": {"sha1": commit}, "path_in_vcs": "crates/example"}
        vcs_raw = json.dumps(vcs).encode()
        (crate / ".cargo_vcs_info.json").write_bytes(vcs_raw)
        body = b"Fixture license body, not a real license.\n"
        (directory / "LICENSE.txt").write_bytes(body)
        entry = {
            "name": "example", "version": "1.0.0", "source": "crates.io", "license": "MIT",
            "repository": "https://github.com/example/project", "vcs_commit": commit,
            "path_in_vcs": "crates/example", "file": "LICENSE.txt", "sha256": sha256(body),
            "upstream_path": "LICENSE", "upstream_url": prefix + "LICENSE",
            "provenance": {"crate_manifest_url": prefix + "crates/example/Cargo.toml",
                           "crate_manifest_sha256": "b" * 64,
                           "registry_vcs_info_sha256": sha256(vcs_raw)},
        }
        manifest = {"schema_version": 1, "legal_clearance": False, "supplements": [entry]}
        (directory / "manifest.json").write_text(json.dumps(manifest))
        package = {"id": "example", "name": "example", "version": "1.0.0", "license": "MIT",
                   "source": "registry+https://github.com/rust-lang/crates.io-index",
                   "repository": entry["repository"], "manifest_path": str(crate / "Cargo.toml")}
        return crate, directory, package, manifest, vcs_raw

    def test_pinned_supplement_closes_only_exact_identity_and_records_public_provenance(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            crate, directory, package, manifest, vcs_raw = self.supplement_fixture(root)
            supplements = load_supplements(directory)
            record, body = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertEqual(record["unresolved"], [])
            self.assertIn("Fixture license body", body)
            file = record["license_files"][0]
            self.assertEqual(file["sha256"], manifest["supplements"][0]["sha256"])
            self.assertEqual(file["provenance"]["vcs_commit"], "a" * 40)
            self.assertIn(file["provenance"]["upstream_url"], body)
            self.assertNotIn(str(root), json.dumps(record) + body)
            self.assertEqual(set(TARGETS), {"aarch64-apple-darwin", "x86_64-apple-darwin",
                                          "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"})
            for change in ({"version": "1.0.1"}, {"repository": "https://github.com/other/project"},
                           {"license": "Apache-2.0"}, {"source": None}):
                with self.subTest(change=change):
                    result, text = package_inventory({**package, **change}, TARGETS, root, set(), supplements)
                    self.assertIn("license_text_missing", result["unresolved"])
                    self.assertEqual(text, "")
            for mutation in (
                lambda vcs: vcs["git"].update(sha1="c" * 40),
                lambda vcs: vcs["git"].update(dirty=True),
                lambda vcs: vcs.update(path_in_vcs="crates/other"),
            ):
                vcs = json.loads(vcs_raw)
                mutation(vcs)
                (crate / ".cargo_vcs_info.json").write_text(json.dumps(vcs))
                result, text = package_inventory(package, TARGETS, root, set(), supplements)
                self.assertIn("license_supplement_vcs_mismatch", result["unresolved"])
                self.assertIn("license_text_missing", result["unresolved"])
                self.assertEqual(text, "")
            (crate / ".cargo_vcs_info.json").unlink()
            result, _ = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("license_supplement_vcs_unavailable", result["unresolved"])

    def test_supplement_tampering_or_external_file_never_closes_missing_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, directory, package, _, _ = self.supplement_fixture(root)
            supplements = load_supplements(directory)
            text_file = directory / "LICENSE.txt"
            text_file.write_text("tampered license")
            record, text = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("license_supplement_digest_mismatch", record["unresolved"])
            self.assertIn("license_text_missing", record["unresolved"])
            self.assertEqual(text, "")
            text_file.unlink()
            record, _ = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("license_text_missing", record["unresolved"])
            external = root / "private-secret"
            external.write_text("private contents")
            text_file.symlink_to(external)
            record, text = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("notice_outside_source:supplements/LICENSE.txt", record["unresolved"])
            self.assertNotIn("private contents", json.dumps(record) + text)

    def test_supplement_manifest_rejects_latest_urls_traversal_and_duplicate_pins(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, directory, _, manifest, _ = self.supplement_fixture(root)
            for mutation in (
                lambda data: data.update(legal_clearance=True),
                lambda data: data["supplements"].append(copy.deepcopy(data["supplements"][0])),
                lambda data: data["supplements"][0].update(file="../private"),
                lambda data: data["supplements"][0].update(upstream_path="../LICENSE"),
                lambda data: data["supplements"][0].update(vcs_commit="main"),
                lambda data: data["supplements"][0].update(sha256="not-a-digest"),
                lambda data: data["supplements"][0].update(upstream_url="https://example.com/latest/LICENSE"),
                lambda data: data["supplements"][0]["provenance"].update(crate_manifest_url="file:///private/source"),
            ):
                data = copy.deepcopy(manifest)
                mutation(data)
                (directory / "manifest.json").write_text(json.dumps(data))
                with self.assertRaisesRegex(RuntimeError, "supplement manifest is invalid"):
                    load_supplements(directory)

    def test_nested_multiple_notices_root_crate_and_pinned_dirty_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            crate, directory, package, manifest, _ = self.supplement_fixture(root)
            entry = manifest["supplements"][0]
            nested = directory / "third-party/example-1.0.0"
            nested.mkdir(parents=True)
            (directory / "LICENSE.txt").rename(nested / "LICENSE.txt")
            entry["file"] = "third-party/example-1.0.0/LICENSE.txt"
            second = b"Second upstream license.\n"
            (nested / "LICENSE-OTHER").write_bytes(second)
            prefix = entry["upstream_url"].removesuffix("LICENSE")
            entry["additional_files"] = [{
                "file": "third-party/example-1.0.0/LICENSE-OTHER",
                "upstream_path": "LICENSE-OTHER", "upstream_url": prefix + "LICENSE-OTHER",
                "sha256": sha256(second),
            }]
            vcs_raw = json.dumps({"git": {"sha1": "a" * 40, "dirty": True},
                                  "path_in_vcs": ""}).encode()
            (crate / ".cargo_vcs_info.json").write_bytes(vcs_raw)
            packaged = b'[package]\nname="example"\nversion="1.0.0"\n'
            (crate / "Cargo.toml.orig").write_bytes(packaged)
            entry.update(path_in_vcs="", vcs_dirty=True)
            entry["provenance"].update(crate_manifest_url=prefix + "Cargo.toml",
                                       registry_vcs_info_sha256=sha256(vcs_raw),
                                       registry_manifest_sha256=sha256(packaged))
            (directory / "manifest.json").write_text(json.dumps(manifest))
            supplements = load_supplements(directory)
            record, text = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertEqual(record["unresolved"], [])
            self.assertEqual(len(record["license_files"]), 2)
            self.assertIn("Second upstream license.", text)
            (crate / "Cargo.toml.orig").write_bytes(packaged + b"# changed\n")
            record, text = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("license_supplement_package_mismatch", record["unresolved"])
            self.assertEqual(text, "")
            (crate / "Cargo.toml.orig").write_bytes(packaged)
            (nested / "LICENSE-OTHER").unlink()
            record, text = package_inventory(package, TARGETS, root, set(), supplements)
            self.assertIn("license_text_missing", record["unresolved"])
            self.assertEqual(text, "")
            entry["additional_files"][0]["file"] = "third-party/../../private"
            (directory / "manifest.json").write_text(json.dumps(manifest))
            with self.assertRaisesRegex(RuntimeError, "supplement manifest is invalid"):
                load_supplements(directory)

    def test_normal_build_closure_excludes_dev_and_unrelated_packages_and_redacts_private_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages = []
            for name in ("oo-cli", "runtime", "build-helper", "dev-only", "unrelated"):
                folder = root / name
                folder.mkdir()
                (folder / "Cargo.toml").write_text('[package]\nlicense = "MIT"\n')
                (folder / "LICENSE-MIT").write_text("Fixture license body; not an upstream license.\n")
                packages.append({"name": name, "version": "1.0.0", "id": f"path+file://{folder}",
                                 "manifest_path": str(folder / "Cargo.toml"), "license": "MIT",
                                 "source": None})
            trees = {
                "aarch64-apple-darwin": f"oo-cli v1.0.0 ({root}/oo-cli)\nruntime v1.0.0\nbuild-helper v1.0.0 (proc-macro)\nruntime v1.0.0 (*)",
                "x86_64-unknown-linux-gnu": "oo-cli v1.0.0\nruntime v1.0.0",
            }
            selected = select_packages({"packages": packages}, trees)
            self.assertEqual([item["package"]["name"] for item in selected], ["build-helper", "oo-cli", "runtime"])
            self.assertEqual(next(item for item in selected if item["package"]["name"] == "runtime")["targets"],
                             set(trees))
            runtime = next(item["package"] for item in selected if item["package"]["name"] == "runtime")
            runtime["source"] = "git+https://private-user:private-secret@internal.example/repo"
            runtime["license"] = f"LicenseRef {root}/private-license"
            runtime["license_file"] = str(root / "private-license")
            vendor = root / "runtime/vendor/aws-lc"
            vendor.mkdir(parents=True)
            (vendor / "NOTICE").write_text("Vendored fixture notice.\n")
            record, text = package_inventory(runtime, set(trees), root, set())
            encoded = json.dumps(record) + text
            for private in (str(root), "private-user", "private-secret", "internal.example", "manifest_path"):
                self.assertNotIn(private, encoded)
            self.assertEqual(record["source"], "git (URL omitted)")
            self.assertIn("declared_license_file_outside_package", record["unresolved"])
            self.assertIn("license_expression_local_path_redacted", record["unresolved"])
            self.assertEqual([item["path"] for item in record["license_files"]],
                             ["LICENSE-MIT", "vendor/aws-lc/NOTICE"])
            self.assertEqual(len(record["text_sha256"]), 64)
            missing = dict(runtime, name="missing", license=None,
                           manifest_path=str(root / "empty/Cargo.toml"), license_file="LICENSE")
            (root / "empty").mkdir()
            record, _ = package_inventory(missing, set(trees), root, set())
            self.assertIn("license_expression_missing", record["unresolved"])
            self.assertIn("license_text_missing", record["unresolved"])
            self.assertIn("notice_missing_or_unreadable:LICENSE", record["unresolved"])
            (root / ".git").mkdir()
            with self.assertRaisesRegex(RuntimeError, "outside Git"):
                outside_git(root / "output")


if __name__ == "__main__":
    unittest.main()
