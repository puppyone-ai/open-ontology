//! One source type for every backend the engine serves: `fs`, and with the
//! `objectstore-s3` feature `s3` (B-S4-01). [`AnyFactory`] picks by
//! `drive.backend`; the jobs stay generic over [`SourceFactory`].

use std::collections::BTreeSet;
use std::path::PathBuf;

use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, ListCursor, ListPage, ObjectMeta,
    ReadExpect,
};

use super::{DriveRef, FsFactory, HashableSource, SourceFactory};

/// A Drive source of either kind.
#[derive(Clone)]
pub enum AnySource {
    Fs(FsSource),
    #[cfg(feature = "objectstore-s3")]
    S3(oo_connectors::objectstore_s3::opendal::S3Source),
}

impl DocumentSource for AnySource {
    fn backend(&self) -> DocBackend {
        match self {
            AnySource::Fs(s) => s.backend(),
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.backend(),
        }
    }

    fn capabilities(&self) -> BTreeSet<DocCapability> {
        match self {
            AnySource::Fs(s) => s.capabilities(),
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.capabilities(),
        }
    }

    async fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        match self {
            AnySource::Fs(s) => s.list(cursor, page_size).await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.list(cursor, page_size).await,
        }
    }

    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        match self {
            AnySource::Fs(s) => s.stat(raw_key).await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.stat(raw_key).await,
        }
    }

    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        match self {
            AnySource::Fs(s) => s.read(raw_key, offset, len, expect).await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.read(raw_key, offset, len, expect).await,
        }
    }
}

impl HashableSource for AnySource {
    async fn list_scopes(&self) -> Result<Option<Vec<String>>, DocSourceError> {
        match self {
            AnySource::Fs(s) => s.list_scopes().await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.list_scopes().await,
        }
    }

    async fn list_scope(
        &self,
        scope: &str,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        match self {
            AnySource::Fs(s) => s.list_scope(scope, cursor, page_size).await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.list_scope(scope, cursor, page_size).await,
        }
    }

    fn any_source(&self) -> Option<&AnySource> {
        Some(self)
    }

    async fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> Result<(DualDigest, ObjectMeta), DocSourceError> {
        match self {
            AnySource::Fs(s) => s.hash_object(raw_key).await,
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => s.stream_object(raw_key, |_| Ok(())).await,
        }
    }
}

/// Opens `fs` Drives always and `s3` Drives when built with `objectstore-s3`.
#[derive(Clone, Debug)]
pub struct AnyFactory {
    /// Where named `secret_ref`s live (`<state>/home/secrets`, read only).
    pub secrets_dir: PathBuf,
}

impl AnyFactory {
    /// `OO_DOCS_SECRETS_DIR`, else `$HOME/.config/open-ontology/secrets`.
    pub fn from_env() -> Self {
        let dir = std::env::var_os("OO_DOCS_SECRETS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".config/open-ontology/secrets")
            });
        Self { secrets_dir: dir }
    }
}

impl SourceFactory for AnyFactory {
    type Source = AnySource;

    fn open(&self, drive: &DriveRef) -> Result<AnySource, DocSourceError> {
        match drive.backend.as_str() {
            "fs" => FsFactory.open(drive).map(AnySource::Fs),
            #[cfg(feature = "objectstore-s3")]
            // oss, cos and r2 are the same S3 compatible path with the cloud's endpoint
            // and addressing defaults (10 4.3.7, B-S11-01).
            "s3" | "oss" | "cos" | "r2" => {
                use oo_connectors::objectstore_s3::opendal::{S3Config, S3Source, SecretResolver};
                let cfg = S3Config::from_drive_config_for(
                    &drive.backend,
                    &drive.config,
                    &SecretResolver {
                        secrets_dir: self.secrets_dir.clone(),
                    },
                )?;
                S3Source::open(&cfg).map(AnySource::S3)
            }
            _ => Err(DocSourceError::InvalidConfig("backend is not served")),
        }
    }
}

/// One piece of a streamed download (see [`AnySource::stream_object`]).
#[derive(Debug)]
pub enum Piece {
    /// The next bytes of the object.
    Bytes(Vec<u8>),
    /// Everything sent so far is void; the object is read again from the start.
    Restart,
}

impl AnySource {
    /// True for sources whose originals are fetched over a network and so are
    /// cached in the CAS (`orig/`). `fs` Drives read the file in place.
    pub fn is_remote(&self) -> bool {
        !matches!(self, AnySource::Fs(_))
    }

    /// Stream the whole object in ranges with `If-Match`; memory is one range.
    /// `Changed` means it moved underneath the read and the caller starts over.
    /// Only remote sources stream (an `fs` Drive is read in place).
    pub async fn stream_object<F>(
        &self,
        raw_key: &[u8],
        mut sink: F,
    ) -> Result<(DualDigest, ObjectMeta), DocSourceError>
    where
        F: FnMut(Piece) -> Result<(), DocSourceError> + Send,
    {
        match self {
            AnySource::Fs(_) => {
                let _ = (raw_key, &mut sink);
                Err(DocSourceError::InvalidConfig(
                    "fs objects are read in place",
                ))
            }
            #[cfg(feature = "objectstore-s3")]
            AnySource::S3(s) => {
                use oo_connectors::objectstore_s3::opendal::Streamed;
                s.stream_object(raw_key, move |p| {
                    sink(match p {
                        Streamed::Bytes(b) => Piece::Bytes(b),
                        Streamed::Restart => Piece::Restart,
                    })
                })
                .await
            }
        }
    }
}
