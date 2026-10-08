//! Length-prefixed AAD. A copied row fails open under another namespace.

use crate::error::Fault;
use crate::ids::{deployment_ok, ident_ok};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub deployment: String,
    pub project: String,
    pub namespace: String,
    pub secret_id: String,
}

impl Scope {
    pub fn reference(&self) -> String {
        format!("secret://{}/{}/{}/{}", self.deployment, self.project, self.namespace, self.secret_id)
    }

    pub fn parse_ref(value: &str) -> Result<Self, Fault> {
        let Some(rest) = value.strip_prefix("secret://") else {
            return Err(Fault::malformed());
        };
        let mut parts = rest.split('/');
        let deployment = parts.next().ok_or_else(Fault::malformed)?;
        let project = parts.next().ok_or_else(Fault::malformed)?;
        let namespace = parts.next().ok_or_else(Fault::malformed)?;
        let secret_id = parts.next().ok_or_else(Fault::malformed)?;
        if parts.next().is_some() || !deployment_ok(deployment) || !ident_ok(project) || !ident_ok(namespace) || !ident_ok(secret_id) {
            return Err(Fault::malformed());
        }
        Ok(Self {
            deployment: deployment.to_owned(),
            project: project.to_owned(),
            namespace: namespace.to_owned(),
            secret_id: secret_id.to_owned(),
        })
    }
}

pub fn canonical(scope: &Scope, version: i32) -> Vec<u8> {
    let version_text = version.to_string();
    let mut out = Vec::with_capacity(128);
    for field in [
        b"oo.secret-control/aad/v1".as_slice(),
        scope.deployment.as_bytes(),
        scope.project.as_bytes(),
        scope.namespace.as_bytes(),
        scope.secret_id.as_bytes(),
        version_text.as_bytes(),
    ] {
        let len = u16::try_from(field.len()).unwrap_or(0);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}
