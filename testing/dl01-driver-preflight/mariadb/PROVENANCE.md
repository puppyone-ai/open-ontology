# Provenance

Dependency version pins come from `tools/mysql-preview/Cargo.lock` at
`c6f628a3a8c34c731704e2008d31bf00ec6a7f9a`. The checked packages are
`mysql_async` 0.36.1, `mysql_common` 0.35.5, Arrow series 60.0.0, `tokio`
1.53.1, `serde` 1.0.229, and `serde_json` 1.0.151. Arrow 60.0.0 is also the
series in base `577160c22de63adb9b38e0147f3bc2da893c4d9d`. No root lock was
updated.

No Rust module from that MySQL preview was copied. `tools/mysql-preview/src/source.rs`
rejects a MariaDB `version_comment` before it reads rows. This adapter requires
`VERSION()` to start with `11.4.13-MariaDB` and the comment to name MariaDB.
`tinyint` stays an integer. JSON stays MariaDB `longtext` plus `JSON_VALID`.

The date conversion is Howard Hinnant's civil-from-days algorithm, written
here, and pinned to the Unix epoch in unit tests. It is not a MySQL type map.
