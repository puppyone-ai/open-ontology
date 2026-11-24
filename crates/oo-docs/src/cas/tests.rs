use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use super::layout::*;
use super::*;

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "oo-docs-cas-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&d);
        Tmp(d.join("cas"))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn b3(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn tree_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            tree_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn cas_layout_paths_are_valid_and_injective() {
    let a = "ab".repeat(32);
    let b = format!("ab{}", "cd".repeat(31) + "ce");
    assert_eq!(
        object_rel_path(CasKind::Orig, &a).unwrap(),
        format!("orig/b3/ab/ab/{a}")
    );
    assert_eq!(
        object_rel_path(CasKind::Ir, &a).unwrap(),
        format!("ir/ab/{a}.json")
    );
    assert_eq!(
        object_rel_path(CasKind::Llm, &a).unwrap(),
        format!("llm/ab/{a}.json")
    );
    assert_ne!(
        object_rel_path(CasKind::Ir, &a),
        object_rel_path(CasKind::Ir, &b)
    );
    for bad in [
        "",
        "AB",
        "../x",
        &"a".repeat(63),
        &"a".repeat(65),
        &"g".repeat(64),
        &"A".repeat(64),
    ] {
        assert_eq!(object_rel_path(CasKind::Ir, bad), None, "{bad}");
    }
    // Round trip and character set over many keys.
    let mut x = 0x1234_5678_9abc_def0u64;
    for _ in 0..2000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let key = b3(&x.to_le_bytes());
        for kind in CasKind::ALL {
            let rel = object_rel_path(kind, &key).unwrap();
            assert!(
                rel.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/._".contains(&c))
            );
            assert!(!rel.contains(".."));
            let name = rel.rsplit('/').next().unwrap();
            assert_eq!(
                key_from_file_name(kind, name).as_deref(),
                Some(key.as_str())
            );
        }
    }
}

#[test]
fn cas_open_creates_layout_clears_tmp_and_checks_version() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    for d in ["orig/b3", "ir", "llm", "parts", "packs", "tmp"] {
        assert!(t.0.join(d).is_dir(), "{d}");
    }
    assert_eq!(
        fs::read_to_string(t.0.join("VERSION")).unwrap(),
        "oo.docs.cas/v1\n"
    );
    fs::write(t.0.join("tmp/stale.part"), b"half").unwrap();
    drop(cas);
    Cas::open(&t.0).unwrap();
    assert!(!t.0.join("tmp/stale.part").exists());
    fs::write(t.0.join("VERSION"), b"oo.docs.cas/v9\n").unwrap();
    assert_eq!(Cas::open(&t.0).unwrap_err(), CasError::VersionMismatch);
}

#[cfg(unix)]
#[test]
fn cas_modes_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let put = cas.put(CasKind::Ir, b"{}").unwrap();
    let file = t.0.join(object_rel_path(CasKind::Ir, &put.b3).unwrap());
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&t.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(t.0.join("ir")).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn cas_put_get_roundtrip_dedupe_and_tmp_empty() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let data = b"hello cas";
    let p = cas.put(CasKind::Llm, data).unwrap();
    assert_eq!(p.b3, b3(data));
    assert!(p.created && !p.repaired && p.size == 9);
    assert_eq!(cas.get(CasKind::Llm, &p.b3).unwrap(), data);
    let again = cas.put(CasKind::Llm, data).unwrap();
    assert!(!again.created);
    assert_eq!(fs::read_dir(t.0.join("tmp")).unwrap().count(), 0);
    assert!(cas.exists(CasKind::Llm, &p.b3).unwrap());
    assert!(!cas.exists(CasKind::Ir, &p.b3).unwrap());
    assert_eq!(cas.get(CasKind::Ir, &p.b3), Err(CasError::NotFound));
    assert_eq!(cas.get(CasKind::Ir, "zz"), Err(CasError::BadKey));
    // streaming input larger than the copy buffer
    let big = vec![5u8; 300_000];
    let q = cas.put_reader(CasKind::Orig, &big[..]).unwrap();
    assert_eq!(q.size, 300_000);
    assert_eq!(cas.get(CasKind::Orig, &q.b3).unwrap(), big);
    cas.verify(CasKind::Orig, &q.b3).unwrap();
}

#[test]
fn cas_read_detects_corruption_deletes_and_codes() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let p = cas.put(CasKind::Ir, b"{\"a\":1}").unwrap();
    let file = t.0.join(object_rel_path(CasKind::Ir, &p.b3).unwrap());
    fs::write(&file, b"{\"a\":2}").unwrap();
    let err = cas.get(CasKind::Ir, &p.b3).unwrap_err();
    assert_eq!(
        err,
        CasError::Corrupt {
            kind: CasKind::Ir,
            b3: p.b3.clone()
        }
    );
    assert_eq!(err.code(), "DOCS_ENGINE_CAS_CORRUPT");
    assert!(!file.exists(), "corrupt object is deleted");
    // verify() path
    let p = cas.put(CasKind::Ir, b"{\"a\":1}").unwrap();
    fs::write(&file, b"x").unwrap();
    assert!(matches!(
        cas.verify(CasKind::Ir, &p.b3),
        Err(CasError::Corrupt { .. })
    ));
    assert!(!file.exists());
}

#[test]
fn cas_put_repairs_a_corrupt_existing_object() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let p = cas.put(CasKind::Ir, b"good").unwrap();
    let file = t.0.join(object_rel_path(CasKind::Ir, &p.b3).unwrap());
    fs::write(&file, b"bad!").unwrap();
    let again = cas.put(CasKind::Ir, b"good").unwrap();
    assert!(again.created && again.repaired);
    assert_eq!(cas.get(CasKind::Ir, &p.b3).unwrap(), b"good");
}

#[test]
fn cas_crash_mid_write_leaves_no_partial_object() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let data = vec![9u8; 200_000];
    // killed after 70 KB reached tmp/
    assert_eq!(
        cas.write_object(CasKind::Orig, &data[..], Some(CrashAt::MidWrite(70_000)))
            .unwrap(),
        None
    );
    // killed after fsync, before rename
    assert_eq!(
        cas.write_object(CasKind::Ir, &data[..], Some(CrashAt::BeforeRename))
            .unwrap(),
        None
    );
    let mut objects = Vec::new();
    for kind in CasKind::ALL {
        tree_files(
            &t.0.join(if kind == CasKind::Orig {
                "orig/b3"
            } else {
                kind.dir()
            }),
            &mut objects,
        );
    }
    assert!(
        objects.is_empty(),
        "no object may exist after a crash: {objects:?}"
    );
    let leftovers: Vec<_> = fs::read_dir(t.0.join("tmp")).unwrap().flatten().collect();
    assert_eq!(leftovers.len(), 2);
    assert!(
        leftovers
            .iter()
            .any(|e| e.metadata().unwrap().len() == 70_000)
    );
    assert_eq!(cas.get(CasKind::Ir, &b3(&data)), Err(CasError::NotFound));
    // restart clears the debris and the same write then succeeds
    let cas = Cas::open(&t.0).unwrap();
    assert_eq!(fs::read_dir(t.0.join("tmp")).unwrap().count(), 0);
    assert!(cas.put_reader(CasKind::Orig, &data[..]).unwrap().created);
}

#[test]
fn cas_failing_reader_removes_its_tmp_file() {
    struct Boom(u8);
    impl Read for Boom {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("boom"));
            }
            self.0 -= 1;
            buf[0] = 1;
            Ok(1)
        }
    }
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    assert!(cas.put_reader(CasKind::Ir, Boom(3)).is_err());
    assert_eq!(fs::read_dir(t.0.join("tmp")).unwrap().count(), 0);
}

#[test]
fn cas_gc_marks_sweeps_and_spares_young_objects() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let keep = cas.put(CasKind::Ir, b"keep").unwrap().b3;
    let drop_old = cas.put(CasKind::Ir, b"drop").unwrap().b3;
    let drop_llm = cas.put(CasKind::Llm, b"llm").unwrap().b3;
    let mut refs = RefSet::new();
    refs.insert((CasKind::Ir, keep.clone()));
    // same hash referenced under another kind does not protect this one
    refs.insert((CasKind::Llm, keep.clone()));
    let now = SystemTime::now();
    let young = cas.gc(&refs, now, Duration::from_secs(3600)).unwrap();
    assert_eq!(
        (young.deleted, young.kept_young, young.kept_referenced),
        (0, 2, 1)
    );
    let later = now + Duration::from_secs(7200);
    let r = cas.gc(&refs, later, Duration::from_secs(3600)).unwrap();
    assert_eq!((r.scanned, r.deleted, r.kept_referenced), (3, 2, 1));
    assert!(r.bytes_freed > 0);
    assert!(cas.exists(CasKind::Ir, &keep).unwrap());
    assert!(!cas.exists(CasKind::Ir, &drop_old).unwrap());
    assert!(!cas.exists(CasKind::Llm, &drop_llm).unwrap());
    assert!(
        !t.0.join(format!("llm/{}", &drop_llm[..2])).exists(),
        "empty shard dirs are pruned"
    );
}

#[test]
fn cas_lru_decision_is_deterministic_and_respects_protection() {
    let item = |k: &str, size, t| LruItem {
        key: k.into(),
        size,
        last_used_ns: t,
    };
    let items = vec![
        item("c", 10, 3),
        item("a", 10, 1),
        item("b", 10, 1),
        item("d", 10, 4),
    ];
    assert_eq!(lru_victims(&items, 40, &|_| false), Vec::<String>::new());
    assert_eq!(lru_victims(&items, 25, &|_| false), ["a", "b"]);
    assert_eq!(lru_victims(&items, 25, &|k| k == "a"), ["b", "c"]);
    assert_eq!(lru_victims(&items, 0, &|_| true), Vec::<String>::new());
    assert!(gc_should_delete(
        false,
        Duration::from_secs(3600),
        Duration::from_secs(3600)
    ));
    assert!(!gc_should_delete(
        true,
        Duration::from_secs(9999),
        Duration::from_secs(3600)
    ));
}

#[test]
fn cas_orig_eviction_keeps_budget_and_uses_read_recency() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    let mut keys = Vec::new();
    for i in 0..4u8 {
        let k = cas.put(CasKind::Orig, &vec![i; 1000]).unwrap().b3;
        let f = t.0.join(object_rel_path(CasKind::Orig, &k).unwrap());
        let when = SystemTime::now() - Duration::from_secs(1000 - u64::from(i) * 100);
        File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(when)
            .unwrap();
        keys.push(k);
    }
    // a verified read refreshes recency: oldest object becomes newest
    cas.get(CasKind::Orig, &keys[0]).unwrap();
    let r = cas.evict_orig(2500, &BTreeSet::new()).unwrap();
    assert_eq!(r.bytes_before, 4000);
    assert_eq!(r.bytes_after, 2000);
    assert_eq!(r.evicted, [keys[1].clone(), keys[2].clone()]);
    assert!(cas.exists(CasKind::Orig, &keys[0]).unwrap());
    assert!(cas.exists(CasKind::Orig, &keys[3]).unwrap());
    assert_eq!(cas.usage().orig_bytes, 2000);
    // protected objects survive even over budget
    let prot: BTreeSet<String> = [keys[0].clone(), keys[3].clone()].into();
    let r = cas.evict_orig(0, &prot).unwrap();
    assert!(r.evicted.is_empty());
}

#[test]
fn sample_verify_finds_a_damaged_object_and_deletes_nothing() {
    let t = Tmp::new();
    let cas = Cas::open(&t.0).unwrap();
    assert_eq!(
        cas.sample_verify(20, 0),
        gc::SampleReport::default(),
        "empty store"
    );
    let mut keys = Vec::new();
    for i in 0..30u8 {
        keys.push(cas.put(CasKind::Ir, &[i; 40]).unwrap().b3);
    }
    let r = cas.sample_verify(20, 7);
    assert_eq!((r.checked, r.corrupt), (20, 0));
    // Damage one object in place (same name, other bytes).
    let path = cas.abs(CasKind::Ir, &keys[3]).unwrap();
    fs::write(&path, b"damaged").unwrap();
    // Sampling every object finds it; the file is still there afterwards.
    let r = cas.sample_verify(100, 0);
    assert_eq!((r.checked, r.corrupt), (30, 1));
    assert!(path.exists(), "a read only check removes nothing");
    // Another seed samples a different start but never more than asked.
    assert_eq!(cas.sample_verify(5, 11).checked, 5);
}
