// formal: main=file_lifecycle inv=inv steps=40 verify-steps=10 witnesses="witPublished witDuplicate witSupersededMidParse witStaleDiscarded witDeletedRestored witOutdatedReparse witQuarantined witQuarantineRetried witUnreadable witState_stored witState_queued witState_parsing witState_parsed witState_extracting witState_indexed witState_published witState_parse_failed witState_extract_partial witState_unsupported witState_too_large witState_encrypted witState_duplicate witState_superseded witState_deleted witState_outdated" vinv="invOnlyTableTransitions invPublishedFromLatest invNoStaleOverwrite invDuplicateSameHash invNoDeadEnd invQuarantineShape invQuarantinePoison"
// FM2: file state machine (00 5.1, 10 4.2), checked against the single
// transition table `is_legal` in crates/oo-docs/src/state.rs. Scope: all 16
// states, the "any state" lifecycle rows, quarantine (a parse_failed file whose
// blob was quarantined, 10 4.1.6) and manual retry, 3 files, 3 versions per
// file, deletes. V-S2-02 was the md subset; V-S3-01 (this file) is the full
// machine. Rename does not touch state and is not modelled. The table below is
// an independent copy of 10 4.2 plus the `stored -> parse_failed` row of 10
// 4.3.2 (unreadable file); conformance_state.rs compares it with `is_legal`
// for every pair. Mapping and open points: formal/reviews/FM2.md.
module file_lifecycle {
  pure val F = 3
  pure val V = 3       // versions per file (new blob / re-appearance bumps `ver`)
  pure val H = 2       // content hashes (small, so duplicates occur)
  pure val FILES: Set[int] = 0.to(F - 1)
  pure val HASHES: Set[int] = 0.to(H - 1)

  // pfc: why the file is in parse_failed ("" otherwise): "quarantined" (poison blob), "exhausted"
  // (attempts used up or not retryable), "unreadable" (stored, object could not be read)
  type File = { st: str, ver: int, hash: int, jobVer: int, resVer: int, pubVer: int, dupHash: int, pfc: str }
  pure val NEW: File = { st: "absent", ver: 0, hash: -1, jobVer: -1, resVer: -1, pubVer: -1, dupHash: -1, pfc: "" }

  var files: int -> File
  var lastFrom: str
  var lastTo: str
  var discards: int      // late results of an older version that were dropped
  var poison: Set[int]   // content hashes whose parse job was quarantined (poison blobs)
  var retried: bool      // witness: a quarantined file was manually retried

  // 10 4.2 table, EDGES in state.rs. `absent` is "no row": only absent -> stored.
  pure val EDGES: Set[(str, str)] = Set(
    ("stored", "too_large"), ("stored", "unsupported"), ("stored", "queued"), ("stored", "duplicate"), ("stored", "parse_failed"),
    ("queued", "unsupported"), ("queued", "encrypted"), ("queued", "parsing"),
    ("parsing", "parsed"), ("parsing", "queued"), ("parsing", "parse_failed"),
    ("parsed", "extracting"),
    ("extracting", "indexed"), ("extracting", "extract_partial"),
    ("indexed", "published"), ("indexed", "outdated"),
    ("extract_partial", "published"), ("extract_partial", "queued"), ("extract_partial", "outdated"),
    ("published", "outdated"),
    ("parse_failed", "queued"), ("parse_failed", "outdated"),
    ("unsupported", "outdated"), ("duplicate", "outdated"),
    ("outdated", "parsing"),
    ("superseded", "stored"), ("deleted", "stored")
  )
  pure def legal(from: str, dst: str): bool =
    if (from == "absent") dst == "stored"
    else if (dst == "superseded" or dst == "deleted")
      (from != "superseded" and from != "deleted") or (from == "superseded" and dst == "deleted")
    else EDGES.contains((from, dst))

  pure def completed(s: str): bool = s == "indexed" or s == "published"
  pure def hasBlob(m: int -> File, f: int): bool =
    FILES.exists(g => g != f and m.get(g).hash == m.get(f).hash and completed(m.get(g).st))

  action setFile(f: int, nf: File, from: str, dst: str): bool = all {
    legal(from, dst),
    discards' = discards,
    files' = files.set(f, { ...nf, pfc: if (dst == "parse_failed") nf.pfc else "" }),
    lastFrom' = from,
    lastTo' = dst,
    poison' = if (dst == "parse_failed" and nf.pfc == "quarantined") poison.union(Set(nf.hash)) else poison,
    retried' = (retried or (from == "parse_failed" and dst == "queued" and files.get(f).pfc == "quarantined")),
  }

  action init = all {
    files' = FILES.mapBy(i => NEW), lastFrom' = "absent", lastTo' = "absent", discards' = 0,
    poison' = Set(), retried' = false,
  }

  // listing finds a new path, fingerprint stable
  action create(f: int): bool = all {
    files.get(f).st == "absent",
    nondet h = HASHES.oneOf()
    setFile(f, { ...NEW, st: "stored", ver: 1, hash: h }, "absent", "stored"),
  }

  // moves that carry no extra data: side states, parse retries, extract results
  action plain(f: int, dst: str): bool = {
    val fl = files.get(f)
    all {
      fl.st != "absent",
      dst != "duplicate" and dst != "queued" and dst != "parsing" and dst != "parsed" and dst != "published"
        and dst != "outdated" and dst != "superseded" and dst != "deleted" and dst != "stored",
      setFile(f, { ...fl, st: dst }, fl.st, dst),
    }
  }

  // parse gave up: attempts used up or not retryable or quarantined (from parsing), or the object was
  // unreadable (from stored, 10 4.3.2). A quarantined blob becomes poison for every path carrying it.
  action failParse(f: int, code: str): bool = {
    val fl = files.get(f)
    all {
      fl.st == "parsing" or (fl.st == "stored" and code == "unreadable"),
      code == "unreadable" implies fl.st == "stored",
      setFile(f, { ...fl, st: "parse_failed", pfc: code }, fl.st, "parse_failed"),
    }
  }

  // stored -> duplicate iff the blob already has a completed parse+extract; else stored -> queued
  action toDuplicate(f: int): bool = {
    val fl = files.get(f)
    all { fl.st == "stored", hasBlob(files, f), setFile(f, { ...fl, st: "duplicate", dupHash: fl.hash }, "stored", "duplicate") }
  }
  // queued is reached from stored (new blob only), parsing (retry), extract_partial / parse_failed (manual retry)
  action toQueued(f: int): bool = {
    val fl = files.get(f)
    all {
      fl.st == "stored" implies not(hasBlob(files, f)),
      // manual retry of a poisoned file (pfc quarantined) is allowed: the pump may fail it again at once
      setFile(f, { ...fl, st: "queued" }, fl.st, "queued"),
    }
  }
  // the parse job is leased for the version the file is at right now
  action toParsing(f: int): bool = {
    val fl = files.get(f)
    setFile(f, { ...fl, st: "parsing", jobVer: fl.ver }, fl.st, "parsing")
  }
  // parse completion: only a result for the current version is stored, anything else is discarded
  action finishParse(f: int): bool = {
    val fl = files.get(f)
    if (fl.st == "parsing" and fl.jobVer == fl.ver) setFile(f, { ...fl, st: "parsed", resVer: fl.ver }, "parsing", "parsed")
    else all {
      fl.jobVer != -1 and (fl.st != "parsing" or fl.jobVer != fl.ver),
      files' = files.set(f, { ...fl, jobVer: -1 }),
      discards' = discards + 1,
      lastFrom' = lastFrom,
      lastTo' = lastTo,
      poison' = poison, retried' = retried,
    }
  }
  // publish: only a result of the current version feeds facts
  action toPublished(f: int): bool = {
    val fl = files.get(f)
    all { fl.resVer == fl.ver, setFile(f, { ...fl, st: "published", pubVer: fl.ver }, fl.st, "published") }
  }
  // parser or prompt upgrade; published facts stay valid meanwhile (D23)
  action toOutdated(f: int): bool = {
    val fl = files.get(f)
    setFile(f, { ...fl, st: "outdated" }, fl.st, "outdated")
  }
  // a new blob at the same path: facts of the old version are withdrawn, a job may still be running
  action supersede(f: int): bool = {
    val fl = files.get(f)
    all {
      fl.ver < V,
      nondet h = HASHES.oneOf()
      setFile(f, { ...fl, st: "superseded", ver: fl.ver + 1, hash: h, pubVer: -1, dupHash: -1 }, fl.st, "superseded"),
    }
  }
  action remove(f: int): bool = {
    val fl = files.get(f)
    setFile(f, { ...fl, st: "deleted", pubVer: -1, dupHash: -1 }, fl.st, "deleted")
  }
  // superseded -> stored (new version enters), deleted -> stored (path re-appears as a new version)
  action restore(f: int): bool = {
    val fl = files.get(f)
    val nv = if (fl.st == "deleted") fl.ver + 1 else fl.ver
    all {
      fl.st == "superseded" or fl.st == "deleted",
      nv <= V,
      nondet h = HASHES.oneOf()
      setFile(f, { ...fl, st: "stored", ver: nv, hash: if (fl.st == "deleted") h else fl.hash }, fl.st, "stored"),
    }
  }

  action step = any {
    { nondet f = FILES.oneOf(); create(f) },
    { nondet f = FILES.oneOf();
      nondet dst = Set("too_large", "unsupported", "encrypted", "extracting", "indexed", "extract_partial").oneOf();
      plain(f, dst) },
    { nondet f = FILES.oneOf(); nondet code = Set("quarantined", "exhausted", "unreadable").oneOf(); failParse(f, code) },
    { nondet f = FILES.oneOf(); toDuplicate(f) },
    { nondet f = FILES.oneOf(); toQueued(f) },
    { nondet f = FILES.oneOf(); toParsing(f) },
    { nondet f = FILES.oneOf(); finishParse(f) },
    { nondet f = FILES.oneOf(); toPublished(f) },
    { nondet f = FILES.oneOf(); toOutdated(f) },
    { nondet f = FILES.oneOf(); supersede(f) },
    { nondet f = FILES.oneOf(); remove(f) },
    { nondet f = FILES.oneOf(); restore(f) },
  }

  // ---- invariants ----
  // only table transitions happen (the table is the independent EDGES copy above)
  val invOnlyTableTransitions = (lastFrom == "absent" and lastTo == "absent") or legal(lastFrom, lastTo)
  // published facts come only from the latest, live file version
  val invPublishedFromLatest = FILES.forall(f => {
    val fl = files.get(f)
    fl.pubVer != -1 implies (fl.pubVer == fl.ver and fl.st != "deleted" and fl.st != "superseded" and fl.st != "absent")
  })
  // a result of an older version never sits under a newer version's state
  val invNoStaleOverwrite = FILES.forall(f => {
    val fl = files.get(f)
    (fl.st == "parsed" or fl.st == "extracting" or fl.st == "indexed" or fl.st == "extract_partial" or fl.st == "published")
      implies fl.resVer == fl.ver
  })
  // duplicate points only at a version with the same content hash
  val invDuplicateSameHash = FILES.forall(f => files.get(f).st == "duplicate" implies files.get(f).dupHash == files.get(f).hash)
  // no state is a dead end: every state has at least one table successor
  val invNoDeadEnd = FILES.forall(f => {
    val s = files.get(f).st
    s != "absent" implies (Set("stored", "queued", "parsing", "parsed", "extracting", "indexed", "published", "parse_failed",
      "extract_partial", "unsupported", "too_large", "encrypted", "duplicate", "superseded", "deleted", "outdated")
      .exists(t => legal(s, t)))
  })
  // parse_failed carries a reason; no other state does
  val invQuarantineShape = FILES.forall(f => {
    val fl = files.get(f)
    (fl.st == "parse_failed") == (fl.pfc != "")
  })
  // a file quarantined and still failed has its content on the poison list
  val invQuarantinePoison = FILES.forall(f => {
    val fl = files.get(f)
    (fl.st == "parse_failed" and fl.pfc == "quarantined") implies poison.contains(fl.hash)
  })
  val inv = invOnlyTableTransitions and invPublishedFromLatest and invNoStaleOverwrite and invDuplicateSameHash and invNoDeadEnd and invQuarantineShape and invQuarantinePoison

  val witPublished = FILES.exists(f => files.get(f).st == "published")
  val witDuplicate = FILES.exists(f => files.get(f).st == "duplicate")
  val witSupersededMidParse = FILES.exists(f => files.get(f).st == "stored" and files.get(f).jobVer != -1 and files.get(f).jobVer < files.get(f).ver)
  val witStaleDiscarded = discards > 0
  val witDeletedRestored = lastFrom == "deleted" and lastTo == "stored"
  val witOutdatedReparse = FILES.exists(f => files.get(f).st == "parsing" and files.get(f).pubVer != -1)
  val witQuarantined = FILES.exists(f => files.get(f).st == "parse_failed" and files.get(f).pfc == "quarantined")
  val witQuarantineRetried = retried
  val witUnreadable = FILES.exists(f => files.get(f).st == "parse_failed" and files.get(f).pfc == "unreadable")
  val witState_stored = FILES.exists(f => files.get(f).st == "stored")
  val witState_queued = FILES.exists(f => files.get(f).st == "queued")
  val witState_parsing = FILES.exists(f => files.get(f).st == "parsing")
  val witState_parsed = FILES.exists(f => files.get(f).st == "parsed")
  val witState_extracting = FILES.exists(f => files.get(f).st == "extracting")
  val witState_indexed = FILES.exists(f => files.get(f).st == "indexed")
  val witState_published = FILES.exists(f => files.get(f).st == "published")
  val witState_parse_failed = FILES.exists(f => files.get(f).st == "parse_failed")
  val witState_extract_partial = FILES.exists(f => files.get(f).st == "extract_partial")
  val witState_unsupported = FILES.exists(f => files.get(f).st == "unsupported")
  val witState_too_large = FILES.exists(f => files.get(f).st == "too_large")
  val witState_encrypted = FILES.exists(f => files.get(f).st == "encrypted")
  val witState_duplicate = FILES.exists(f => files.get(f).st == "duplicate")
  val witState_superseded = FILES.exists(f => files.get(f).st == "superseded")
  val witState_deleted = FILES.exists(f => files.get(f).st == "deleted")
  val witState_outdated = FILES.exists(f => files.get(f).st == "outdated")
}
