use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;

pub const DOC_MAX: usize = 65_536;
pub const PACK_MAX: usize = 1_048_576;
pub const BEARER_FILE_MAX: usize = 512;
pub const CA_MAX: usize = 65_536;
/// The pinned debug `query-pg` binary is about 153 MiB, above a 128 MiB cap.
pub const BINARY_MAX: usize = 256 * 1024 * 1024;
pub const AUDIT_MAX: usize = 32_768;

/// Linux `O_NOFOLLOW`. A symlink is refused before it is followed.
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0x20000;

pub enum ReadFail {
    Missing,
    Refused,
    Oversize,
    Io,
}

pub fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>, ReadFail> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return Err(ReadFail::Missing),
    };
    if !meta.file_type().is_file() {
        return Err(ReadFail::Refused);
    }
    if meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(ReadFail::Oversize);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    options.custom_flags(O_NOFOLLOW);
    let mut file = options.open(path).map_err(|_| ReadFail::Refused)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|_| ReadFail::Io)?;
    if buf.len() > max {
        return Err(ReadFail::Oversize);
    }
    Ok(buf)
}
