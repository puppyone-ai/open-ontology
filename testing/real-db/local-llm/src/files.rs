use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use crate::fault::Fault;

pub fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>, Fault> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fault::config())?;
    if !meta.file_type().is_file() || meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(Fault::config());
    }
    let mut file = File::open(path).map_err(|_| Fault::io())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|_| Fault::io())?;
    if buf.len() > max {
        return Err(Fault::config());
    }
    Ok(buf)
}

pub fn reject_redirect_env() -> Result<(), Fault> {
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_BASE",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(Fault::config());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn redirect_env_absent_is_config_ok() {
        assert!(super::reject_redirect_env().is_ok());
    }
}
