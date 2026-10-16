use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn require(ok: bool, reason: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(reason.into()) }
}
pub fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(|| format!("field:{key}").into())
}
pub fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("array:{key}").into())
}
pub fn load(p: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
pub fn private(p: impl AsRef<Path>) -> Result<PathBuf> {
    let p = p.as_ref();
    let m = fs::symlink_metadata(p)?;
    let uid = String::from_utf8(Command::new("id").arg("-u").output()?.stdout)?
        .trim()
        .parse::<u32>()?;
    require(
        p.is_absolute()
            && m.is_file()
            && !m.file_type().is_symlink()
            && m.permissions().mode() & 0o077 == 0
            && m.uid() == uid,
        "private_file",
    )?;
    Ok(p.to_path_buf())
}
pub fn dir(p: impl AsRef<Path>) -> Result<()> {
    let p = p.as_ref();
    if p.exists() {
        let m = fs::symlink_metadata(p)?;
        require(
            m.is_dir() && !m.file_type().is_symlink() && m.permissions().mode() & 0o077 == 0,
            "private_directory",
        )?;
    } else {
        fs::create_dir_all(p)?;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn bytes(v: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(v)?)
}
pub fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn digest(p: impl AsRef<Path>) -> Result<String> {
    Ok(sha(&fs::read(p)?))
}
pub fn put(p: impl AsRef<Path>, b: &[u8]) -> Result<()> {
    let p = p.as_ref();
    if p.exists() {
        require(
            !fs::symlink_metadata(p)?.file_type().is_symlink() && fs::read(p)? == b,
            "immutable_output_drift",
        )?;
        return Ok(());
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    f.write_all(b)?;
    f.sync_all()?;
    Ok(())
}
pub fn save(p: impl AsRef<Path>, v: &Value) -> Result<()> {
    let mut b = bytes(v)?;
    b.push(b'\n');
    put(p, &b)
}
pub fn copy(a: impl AsRef<Path>, b: impl AsRef<Path>) -> Result<()> {
    put(b, &fs::read(a)?)
}
pub fn ident(s: &str) -> Result<&str> {
    require(
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "identifier",
    )?;
    Ok(s)
}
pub fn binary(p: &str) -> Result<()> {
    require(
        Path::new(p)
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            == Some(&digest(p)?),
        "binary_pin",
    )
}
pub fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}

pub struct Calls {
    pub out: PathBuf,
    n: usize,
}
impl Calls {
    pub fn new(out: &Path) -> Result<Self> {
        dir(out)?;
        let n = fs::read_dir(out)?.count();
        Ok(Self { out: out.into(), n })
    }
    pub fn run(&mut self, label: &str, args: &[String], input: Option<&[u8]>) -> Result<Vec<u8>> {
        self.n += 1;
        let prefix = self.out.join(format!("{:04}-{label}", self.n));
        save(
            prefix.with_extension("started.json"),
            &json!({"argv":args,"input_sha256":input.map(sha)}),
        )?;
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(prefix.with_extension("stdout"))?,
            ))
            .stderr(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(prefix.with_extension("stderr"))?,
            ))
            .spawn()?;
        if let Some(data) = input {
            child.stdin.take().ok_or("child_stdin")?.write_all(data)?;
        }
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            require(start.elapsed() < Duration::from_secs(600), "child_deadline")?;
            std::thread::sleep(Duration::from_millis(50));
        };
        save(
            prefix.with_extension("exit.json"),
            &json!({"exit":status.code(),"success":status.success(),"elapsed_ms":start.elapsed().as_millis()}),
        )?;
        require(status.success(), &format!("command_failed:{label}"))?;
        Ok(fs::read(prefix.with_extension("stdout"))?)
    }
    pub fn docker(
        &mut self,
        label: &str,
        container: &str,
        args: &[String],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut argv = strings(&["docker", "exec"]);
        if input.is_some() {
            argv.push("-i".into())
        };
        argv.push(container.into());
        argv.extend_from_slice(args);
        self.run(label, &argv, input)
    }
}
