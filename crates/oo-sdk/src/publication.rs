//! Opt-in F22 transport extension; the frozen JSON query schema is unchanged.

use crate::{LocalError, ProtocolError, QueryError, QueryResponse};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

pub(crate) const EXPECTED: &str = "x-oo-expected-publication-id";
pub(crate) const EXPECTED_VECTOR: &str = "x-oo-expected-data-vector-sha256";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationBinding {
    pub(crate) publication_id: String,
    pub(crate) data_vector_sha256: Option<String>,
}

impl PublicationBinding {
    /// Acquire at the start of a question, then use the receipt for every call.
    /// Reusing `current` across calls does NOT pin a question.
    pub fn current() -> Self {
        Self {
            publication_id: "current".into(),
            data_vector_sha256: None,
        }
    }

    pub fn expected(id: &str, vector: Option<&str>) -> Result<Self, QueryError> {
        if !id.strip_prefix("pub1_").is_some_and(hex64) || vector.is_some_and(|v| !hex64(v)) {
            return Err(QueryError::Local(LocalError::RequestRejected));
        }
        Ok(Self {
            publication_id: id.into(),
            data_vector_sha256: vector.map(str::to_owned),
        })
    }

    pub(crate) fn receipt(&self, headers: &HeaderMap) -> Result<PublicationReceipt, QueryError> {
        let rejected = || QueryError::Protocol(ProtocolError::Rejected);
        let one = |name| -> Result<String, QueryError> {
            let mut values = headers.get_all(name).iter();
            let value = values
                .next()
                .ok_or_else(rejected)?
                .to_str()
                .map_err(|_| rejected())?;
            if values.next().is_some() {
                return Err(rejected());
            }
            Ok(value.into())
        };
        let publication_id = one("x-oo-publication-id")?;
        let data_vector_sha256 = one("x-oo-data-vector-sha256")?;
        Self::expected(&publication_id, Some(&data_vector_sha256)).map_err(|_| rejected())?;
        if (self.publication_id != "current" && self.publication_id != publication_id)
            || self
                .data_vector_sha256
                .as_ref()
                .is_some_and(|v| v != &data_vector_sha256)
        {
            return Err(rejected());
        }
        Ok(PublicationReceipt {
            publication_id,
            data_vector_sha256,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReceipt {
    pub publication_id: String,
    pub data_vector_sha256: String,
}

impl PublicationReceipt {
    pub fn binding(&self) -> Result<PublicationBinding, QueryError> {
        PublicationBinding::expected(&self.publication_id, Some(&self.data_vector_sha256))
    }
}

pub struct QueryReceipt {
    pub response: QueryResponse,
    pub publication_binding: Option<PublicationReceipt>,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
