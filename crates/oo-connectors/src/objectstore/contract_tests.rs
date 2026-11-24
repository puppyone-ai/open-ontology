use std::collections::BTreeSet;

use super::*;

#[test]
fn objectstore_doc_path_rules() {
    for ok in ["a.md", "Projects/a/计划.md", "x/y/z"] {
        assert_eq!(DocPath::new(ok).unwrap().as_str(), ok);
    }
    assert_eq!(DocPath::new(""), Err(DocPathError::Empty));
    for bad in ["/a", "a/", "a//b", "./a", "a/../b", "..", "a\\b"] {
        assert_eq!(DocPath::new(bad), Err(DocPathError::BadSegment), "{bad}");
    }
    assert_eq!(DocPath::new("a\u{7}b"), Err(DocPathError::ControlCharacter));
    assert_eq!(
        DocPath::new(&"x".repeat(256)),
        Err(DocPathError::SegmentTooLong)
    );
    let long = vec!["y".repeat(200); 6].join("/");
    assert_eq!(DocPath::new(&long), Err(DocPathError::TooLong));
    let p = DocPath::new("a/b/c.md").unwrap();
    assert_eq!(p.parent().unwrap().as_str(), "a/b");
    assert_eq!(DocPath::new("c.md").unwrap().parent(), None);
    assert!(serde_json::from_str::<DocPath>("\"../x\"").is_err());
}

#[test]
fn objectstore_errors_are_redacted_and_coded() {
    let all = [
        DocSourceError::NotFound,
        DocSourceError::PermissionDenied,
        DocSourceError::AuthFailed,
        DocSourceError::Throttled { retry_after_ms: 5 },
        DocSourceError::Unavailable,
        DocSourceError::Timeout,
        DocSourceError::Changed,
        DocSourceError::ChecksumMismatch,
        DocSourceError::InvalidConfig("bad config"),
        DocSourceError::Protocol("bad reply"),
        DocSourceError::Io("read failed"),
    ];
    let codes: BTreeSet<&str> = all.iter().map(DocSourceError::code).collect();
    assert_eq!(codes.len(), all.len());
    assert!(codes.iter().all(|c| c.starts_with("DOCS_SOURCE_")));
    assert!(DocSourceError::Throttled { retry_after_ms: 1 }.retryable());
    assert!(!DocSourceError::PermissionDenied.retryable());
}

/// In-memory source used by other boards before real backends exist (70 §3.3).
struct MemSource(Vec<(String, Vec<u8>)>);

impl DocumentSource for MemSource {
    fn backend(&self) -> DocBackend {
        DocBackend::Fs
    }
    fn capabilities(&self) -> BTreeSet<DocCapability> {
        [
            DocCapability::List,
            DocCapability::Stat,
            DocCapability::RangedRead,
        ]
        .into()
    }
    async fn list(
        &self,
        _cursor: Option<&ListCursor>,
        _page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        let entries = self.0.iter().map(|(k, v)| meta(k, v)).collect();
        Ok(ListPage {
            entries,
            next: None,
            complete: true,
        })
    }
    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        Ok(self
            .0
            .iter()
            .find(|(k, _)| k.as_bytes() == raw_key)
            .map(|(k, v)| meta(k, v)))
    }
    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        _expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        let (_, data) = self
            .0
            .iter()
            .find(|(k, _)| k.as_bytes() == raw_key)
            .ok_or(DocSourceError::NotFound)?;
        let start = usize::try_from(offset)
            .map_err(|_| DocSourceError::Io("offset"))?
            .min(data.len());
        let end = start
            .saturating_add(usize::try_from(len).unwrap_or(usize::MAX))
            .min(data.len());
        Ok(data[start..end].to_vec())
    }
}

fn meta(key: &str, data: &[u8]) -> ObjectMeta {
    ObjectMeta {
        raw_key: key.as_bytes().to_vec(),
        path: DocPath::new(key).ok(),
        kind: EntryKind::File,
        size: data.len() as u64,
        etag: None,
        last_modified_ns: Some(0),
        version_id: None,
        crc64: None,
        fs_ino: None,
        fs_ctime_ns: None,
    }
}

#[tokio::test]
async fn objectstore_trait_is_usable_in_memory() {
    let src = MemSource(vec![("a.md".into(), b"hello".to_vec())]);
    let page = src.list(None, 100).await.unwrap();
    assert!(page.complete);
    assert_eq!(page.entries[0].fingerprint().size, 5);
    assert_eq!(
        src.read(b"a.md", 1, 3, &ReadExpect::default())
            .await
            .unwrap(),
        b"ell"
    );
    assert_eq!(src.stat(b"missing").await.unwrap(), None);
    assert_eq!(src.changes(None).await.unwrap(), None);
}

#[test]
fn objectstore_error_codes_exist_in_docs_api_errors() {
    let table: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/docs-api/errors.v1.json"
    ))
    .unwrap();
    let known: BTreeSet<&str> = table["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();
    for err in [
        DocSourceError::NotFound,
        DocSourceError::PermissionDenied,
        DocSourceError::AuthFailed,
        DocSourceError::Throttled { retry_after_ms: 0 },
        DocSourceError::Unavailable,
        DocSourceError::Timeout,
        DocSourceError::Changed,
        DocSourceError::ChecksumMismatch,
        DocSourceError::InvalidConfig(""),
        DocSourceError::Protocol(""),
        DocSourceError::Io(""),
    ] {
        assert!(
            known.contains(err.code()),
            "{} missing from errors.v1.json",
            err.code()
        );
    }
}
