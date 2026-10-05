//! Independent review of the bounded CSV and TSV pull.
//! The object hash is a Safe Rust FIPS 180-4 SHA-256 in this file.
//! It does not call the product hasher and it does not shell out.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::DataType;
use oo_connectors::{
    BomPolicy, DelimitedBudget, DelimitedColumn, DelimitedComplete, DelimitedDialect,
    DelimitedError, DelimitedErrorKind, DelimitedFieldPresence, DelimitedItem, DelimitedProfile,
    DelimitedReader, DelimitedRowLineage, DelimitedType, HeaderPolicy, NullPolicy,
    DELIMITED_PROTOCOL,
};

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const CSV_SHA: &str = "b28a3f046f008a002d875f236a4d1a797abfad9d2dc02bb5c0c0463bb82b0a60";
const TSV_SHA: &str = "09028e8ceb769f47c9e86873c8150db9a62fc2e76869c53c3a001c2e7195d7fe";

const CSV_PREIMAGE: &str = "\
oo-delimited-profile/v1\n\
id=review-a199\n\
dialect=csv\n\
bom=strip\n\
header=exact\n\
null=token\n\
null_token=\\N\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=城市\tutf8\t1\t-\t-\n\
column=on\tbool\t0\t-\t-\n\
column=n\tint64\t0\t-\t-\n\
column=amt\tdecimal\t0\t8\t2\n\
column=note\tutf8\t1\t-\t-\n";

const TSV_PREIMAGE: &str = "\
oo-delimited-profile/v1\n\
id=review-a199-tsv\n\
dialect=tsv\n\
bom=refuse\n\
header=absent\n\
null=empty\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=city\tutf8\t0\t-\t-\n\
column=raw\tutf8\t0\t-\t-\n\
column=n\tint64\t0\t-\t-\n";

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).unwrap().checked_mul(8).unwrap();
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn trusted_sha256(input: &[u8]) -> [u8; 32] {
    assert_eq!(hex(&sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&sha256(b"abc")), ABC_SHA);
    sha256(input)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn config_preimage(profile: &str, digest: &[u8; 32]) -> String {
    let mut body = String::from("oo-delimited-config-identity/v1\n");
    body.push_str(profile);
    body.push_str("expected_sha256=");
    body.push_str(&hex(digest));
    body.push('\n');
    body
}

fn dump(name: &str, bytes: &[u8]) {
    let Ok(dir) = std::env::var("OO_A199_ORACLE_DIR") else {
        return;
    };
    fs::create_dir_all(&dir).unwrap();
    fs::write(format!("{dir}/{name}"), bytes).unwrap();
}

fn col(name: &str, ty: DelimitedType, nullable: bool) -> DelimitedColumn {
    DelimitedColumn::try_new(name, ty, nullable).unwrap()
}

fn budget(
    rows: u32,
    bytes: u64,
    record: u32,
    field: u32,
    total: u64,
    max_rows: u64,
) -> DelimitedBudget {
    DelimitedBudget::try_new(
        rows,
        bytes,
        record,
        field,
        total,
        max_rows,
        8,
        8,
        Instant::now() + Duration::from_secs(30),
    )
    .unwrap()
}

fn wide() -> DelimitedBudget {
    budget(16, 4096, 256, 256, 8192, 100)
}

fn csv_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "review-a199",
        DelimitedDialect::Csv,
        BomPolicy::Strip,
        HeaderPolicy::Exact,
        NullPolicy::Token("\\N".to_owned()),
        vec![
            col("城市", DelimitedType::Utf8, true),
            col("on", DelimitedType::Bool, false),
            col("n", DelimitedType::Int64, false),
            col("amt", DelimitedType::Decimal { precision: 8, scale: 2 }, false),
            col("note", DelimitedType::Utf8, true),
        ],
    )
    .unwrap()
}

fn tsv_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "review-a199-tsv",
        DelimitedDialect::Tsv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![
            col("city", DelimitedType::Utf8, false),
            col("raw", DelimitedType::Utf8, false),
            col("n", DelimitedType::Int64, false),
        ],
    )
    .unwrap()
}

fn one_text(header: HeaderPolicy, bom: BomPolicy, nulls: NullPolicy) -> DelimitedProfile {
    DelimitedProfile::try_new(
        "review-a199-text",
        DelimitedDialect::Csv,
        bom,
        header,
        nulls,
        vec![col("h", DelimitedType::Utf8, true)],
    )
    .unwrap()
}

/// Hand fixture. BOM, exact header, CRLF, a quoted comma, a quoted quote, a
/// quoted LF, int64 extrema, `-0`, and decimal text `1200.50`, `-0.00`, `12.5`.
/// The last record ends at EOF. Offsets below are counted from these bytes.
fn csv_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    out.extend_from_slice("城市,on,n,amt,note\r\n".as_bytes());
    out.extend_from_slice("\"上,海\",true,-9223372036854775808,1200.50,\"甲,\"\"乙\"\"\n丙\"\r\n".as_bytes());
    out.extend_from_slice("北京,false,9223372036854775807,-0.00,\n".as_bytes());
    out.extend_from_slice("\\N,true,-0,12.5,\"\"".as_bytes());
    out
}

fn tsv_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice("熊\t\"a\tb\"\t-9223372036854775808\n".as_bytes());
    out.extend_from_slice("猫,鼠\t\"line\r\n2\"\t9223372036854775807".as_bytes());
    out
}

fn utf8_col(batch: &RecordBatch, index: usize) -> Vec<Option<String>> {
    let array = batch.column(index).as_any().downcast_ref::<StringArray>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row).to_owned()) })
        .collect()
}

fn bool_col(batch: &RecordBatch, index: usize) -> Vec<Option<bool>> {
    let array = batch.column(index).as_any().downcast_ref::<BooleanArray>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn i64_col(batch: &RecordBatch, index: usize) -> Vec<Option<i64>> {
    let array = batch.column(index).as_any().downcast_ref::<Int64Array>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn dec_col(batch: &RecordBatch, index: usize) -> Vec<Option<i128>> {
    let array = batch.column(index).as_any().downcast_ref::<Decimal128Array>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn field<'a>(row: &'a DelimitedRowLineage, name: &str) -> &'a oo_connectors::DelimitedFieldLineage {
    row.fields.iter().find(|item| item.name == name).unwrap()
}

struct Chunked<R> {
    inner: R,
    chunk: usize,
    reads: Arc<AtomicUsize>,
    max_buf: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
}

impl<R: Read> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let _ = self.max_buf.fetch_max(buf.len(), Ordering::SeqCst);
        if buf.is_empty() {
            return Ok(0);
        }
        let limit = self.chunk.min(buf.len());
        let read = self.inner.read(&mut buf[..limit])?;
        self.bytes.fetch_add(read, Ordering::SeqCst);
        Ok(read)
    }
}

fn pull<R: Read>(
    mut reader: DelimitedReader<R>,
) -> (Vec<oo_connectors::DelimitedBatch>, oo_connectors::DelimitedExhausted, DelimitedComplete) {
    let mut batches = Vec::new();
    let exhausted = loop {
        match reader.next_batch().unwrap() {
            DelimitedItem::Batch(batch) => batches.push(*batch),
            DelimitedItem::Exhausted(done) => break done,
        }
    };
    let again = reader.next_batch().unwrap();
    let DelimitedItem::Exhausted(repeated) = again else {
        panic!("a second pull after exhaustion returned a batch");
    };
    assert_eq!(repeated, exhausted);
    let complete = reader.finish().unwrap();
    (batches, exhausted, complete)
}

fn refuse(bytes: &[u8], profile: DelimitedProfile, limit: DelimitedBudget) -> DelimitedError {
    let digest = trusted_sha256(bytes);
    let mut reader = DelimitedReader::open(Cursor::new(bytes.to_vec()), profile, digest, limit).unwrap();
    let err = reader.next_batch().unwrap_err();
    let later = reader.finish().unwrap_err();
    assert_eq!(later, err);
    err
}

fn assert_not_admitted(done: &DelimitedComplete) {
    assert_eq!(done.protocol, DELIMITED_PROTOCOL);
    assert!(!done.admits_import);
    assert!(!done.published);
}

fn hidden(err: &DelimitedError, poison: &str) {
    let shown = format!("{err} | {err:?}");
    assert!(!shown.contains(poison), "{shown}");
}

/// Independent lexical rescale. A larger scale appends zeros. A smaller scale
/// strips trailing zeros. Zero is non-negative.
fn expect_coeff(text: &str, scale: i8) -> i128 {
    assert!(!text.starts_with('+') && !text.contains(' ') && !text.contains('e') && !text.contains('E'));
    let negative = text.starts_with('-');
    let rest = if negative { &text[1..] } else { text };
    let (int_part, frac) = match rest.split_once('.') {
        Some((int_part, frac)) => (int_part, frac),
        None => (rest, ""),
    };
    assert!(!int_part.is_empty());
    let mut digits = String::new();
    digits.push_str(int_part);
    digits.push_str(frac);
    let src_scale = i32::try_from(frac.len()).unwrap();
    while digits.len() > 1 && digits.starts_with('0') {
        digits.remove(0);
    }
    let target = i32::from(scale);
    if target > src_scale {
        let zeros = usize::try_from(target - src_scale).unwrap();
        digits.extend(std::iter::repeat_n('0', zeros));
    } else if target < src_scale {
        let strip = usize::try_from(src_scale - target).unwrap();
        assert!(digits.len() >= strip && digits.ends_with(&"0".repeat(strip)), "{text}");
        digits.truncate(digits.len() - strip);
        if digits.is_empty() {
            digits.push('0');
        }
    }
    while digits.len() > 1 && digits.starts_with('0') {
        digits.remove(0);
    }
    let mut value: i128 = 0;
    for byte in digits.bytes() {
        value = value.checked_mul(10).unwrap().checked_add(i128::from(byte - b'0')).unwrap();
    }
    if negative && digits != "0" {
        value = value.checked_neg().unwrap();
    }
    value
}

fn assert_csv_batch(batch: &oo_connectors::DelimitedBatch, digest: &[u8; 32]) {
    assert_eq!(batch.batch_index, 0);
    assert!(!batch.resumable);
    assert_eq!(batch.raw_bytes, 63 + 40 + 18);
    assert!(batch.arrow_bytes > 0);
    assert_eq!(batch.record_batch.num_rows(), 3);
    assert_eq!(batch.record_batch.num_columns(), 5);
    assert!(!batch.header.admits_import);
    assert!(!batch.header.published);
    assert!(!batch.header.arrow_ipc);
    assert!(!batch.header.executes_native);
    assert_eq!(batch.header.protocol, DELIMITED_PROTOCOL);
    assert_eq!(batch.header.dialect, DelimitedDialect::Csv);
    assert_eq!(batch.header.profile_digest, trusted_sha256(CSV_PREIMAGE.as_bytes()));
    assert_eq!(batch.header.config_identity, trusted_sha256(config_preimage(CSV_PREIMAGE, digest).as_bytes()));
    let note = "甲,\"乙\"\n丙";
    assert_eq!(
        utf8_col(&batch.record_batch, 0),
        vec![Some("上,海".to_owned()), Some("北京".to_owned()), None]
    );
    assert_eq!(bool_col(&batch.record_batch, 1), vec![Some(true), Some(false), Some(true)]);
    assert_eq!(
        i64_col(&batch.record_batch, 2),
        vec![Some(i64::MIN), Some(i64::MAX), Some(0)]
    );
    assert_eq!(expect_coeff("1200.50", 2), 120_050);
    assert_eq!(expect_coeff("-0.00", 2), 0);
    assert_eq!(expect_coeff("12.5", 2), 1_250);
    assert_eq!(dec_col(&batch.record_batch, 3), vec![Some(120_050), Some(0), Some(1_250)]);
    assert_eq!(
        utf8_col(&batch.record_batch, 4),
        vec![Some(note.to_owned()), Some(String::new()), Some(String::new())]
    );
    let schema = batch.record_batch.schema();
    assert_eq!(schema.field(0).name(), "城市");
    assert_eq!(schema.field(3).data_type(), &DataType::Decimal128(8, 2));
    assert_eq!(schema.field(3).metadata().get("oo.delimited.logical").unwrap(), "decimal");
    assert_eq!(schema.field(3).metadata().get("oo.delimited.precision").unwrap(), "8");
    assert_eq!(schema.field(3).metadata().get("oo.delimited.scale").unwrap(), "2");
    assert_eq!(schema.field(0).metadata().get("oo.delimited.protocol").unwrap(), DELIMITED_PROTOCOL);
    assert_eq!(schema.field(1).metadata().get("oo.delimited.logical").unwrap(), "bool");
    assert_eq!(schema.field(2).metadata().get("oo.delimited.logical").unwrap(), "int64");
    let lines = [2_u64, 4, 5];
    let offsets = [25_u64, 88, 128];
    let note_at = [69_u64, 127, 144];
    let note_presence = [
        DelimitedFieldPresence::Value,
        DelimitedFieldPresence::Empty,
        DelimitedFieldPresence::Empty,
    ];
    assert_eq!(batch.lineage.len(), 3);
    for (index, row) in batch.lineage.iter().enumerate() {
        assert_eq!(row.row_index, u64::try_from(index).unwrap());
        assert_eq!(row.line, lines[index]);
        assert_eq!(row.byte_offset, offsets[index]);
        assert_eq!(row.fields.len(), 5);
        assert_eq!(field(row, "note").byte_offset, note_at[index]);
        assert_eq!(field(row, "note").presence, note_presence[index]);
    }
    assert_eq!(field(&batch.lineage[0], "城市").byte_offset, 25);
    assert_eq!(field(&batch.lineage[0], "城市").line, 2);
    assert_eq!(field(&batch.lineage[0], "note").line, 2);
    assert_eq!(field(&batch.lineage[2], "城市").presence, DelimitedFieldPresence::Null);
    assert_eq!(field(&batch.lineage[2], "城市").byte_offset, 128);
    assert_eq!(field(&batch.lineage[1], "note").presence, DelimitedFieldPresence::Empty);
}

#[test]
fn positive_csv_matches_the_hand_fixture_only_at_eof() {
    let bytes = csv_bytes();
    assert_eq!(bytes.len(), 146);
    let digest = trusted_sha256(&bytes);
    assert_eq!(hex(&digest), CSV_SHA);
    let profile = csv_profile();
    assert_eq!(profile.profile_preimage(), CSV_PREIMAGE);
    let expected_config = config_preimage(CSV_PREIMAGE, &digest);
    assert_eq!(profile.config_preimage(digest), expected_config);
    assert_eq!(profile.profile_digest(), trusted_sha256(CSV_PREIMAGE.as_bytes()));
    assert_eq!(profile.config_identity(digest), trusted_sha256(expected_config.as_bytes()));
    assert!(!CSV_PREIMAGE.contains("batch_rows") && !expected_config.contains("batch_bytes"));
    let tight = budget(4, 1024, 256, 256, 4096, 10);
    let loose = budget(8, 2048, 128, 64, 8192, 20);
    assert_eq!(profile.config_identity(digest), csv_profile().config_identity(digest));
    let reader_a = DelimitedReader::open(Cursor::new(bytes.clone()), profile.clone(), digest, tight).unwrap();
    let reader_b = DelimitedReader::open(Cursor::new(bytes.clone()), profile.clone(), digest, loose).unwrap();
    assert_eq!(reader_a.header().config_identity, reader_b.header().config_identity);
    assert_eq!(tight.queue_batches(), 1);
    assert_eq!(loose.queue_batches(), 1);
    drop(reader_a);
    drop(reader_b);

    let (batches, exhausted, done) = pull(
        DelimitedReader::open(Cursor::new(bytes.clone()), profile.clone(), digest, wide()).unwrap(),
    );
    assert_eq!(batches.len(), 1);
    assert_csv_batch(&batches[0], &digest);
    assert_eq!(exhausted.rows, 3);
    assert_eq!(exhausted.batches, 1);
    assert_eq!(exhausted.object_sha256, digest);
    assert_eq!(done.object_sha256, digest);
    assert_eq!(done.rows, 3);
    assert_eq!(done.batches, 1);
    assert_eq!(done.columns, 5);
    assert_eq!(done.profile_id, "review-a199");
    assert_eq!(done.profile_digest, profile.profile_digest());
    assert_not_admitted(&done);
    println!("csv_sha256={}", hex(&digest));
    println!("complete_sha256={}", hex(&done.object_sha256));
    println!("profile_sha256={}", hex(&profile.profile_digest()));
    println!("config_sha256={}", hex(&profile.config_identity(digest)));

    let dir = std::env::temp_dir().join(format!("oo-a199-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("positive.csv");
    fs::write(&path, &bytes).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let max_buf = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(AtomicUsize::new(0));
    let file = File::open(&path).unwrap();
    let chunked = Chunked {
        inner: file,
        chunk: 3,
        reads: Arc::clone(&reads),
        max_buf: Arc::clone(&max_buf),
        bytes: Arc::clone(&got),
    };
    let (streamed, streamed_end, streamed_done) =
        pull(DelimitedReader::open(chunked, profile.clone(), digest, wide()).unwrap());
    assert_eq!(streamed.len(), 1);
    assert_csv_batch(&streamed[0], &digest);
    assert_eq!(streamed_done.object_sha256, digest);
    assert_eq!(streamed_end.object_sha256, digest);
    assert!(max_buf.load(Ordering::SeqCst) <= 256);
    assert!(max_buf.load(Ordering::SeqCst) > 0);
    assert!(reads.load(Ordering::SeqCst) >= 49);
    assert_eq!(got.load(Ordering::SeqCst), bytes.len());
    println!(
        "csv_chunk_reads={} max_buf={} bytes={}",
        reads.load(Ordering::SeqCst),
        max_buf.load(Ordering::SeqCst),
        got.load(Ordering::SeqCst)
    );

    dump("positive.csv", &bytes);
    dump("positive.csv.sha256", format!("{}\n", hex(&digest)).as_bytes());
    dump("profile-preimage.txt", CSV_PREIMAGE.as_bytes());
    dump("config-preimage.txt", expected_config.as_bytes());
    let mut lineage = String::new();
    for row in &batches[0].lineage {
        writeln!(lineage, "row={} line={} offset={}", row.row_index, row.line, row.byte_offset).unwrap();
        for item in &row.fields {
            writeln!(
                lineage,
                "  {} line={} offset={} presence={:?}",
                item.name, item.line, item.byte_offset, item.presence
            )
            .unwrap();
        }
    }
    dump("lineage-csv.txt", lineage.as_bytes());
}

#[test]
fn positive_tsv_matches_the_hand_fixture_only_at_eof() {
    let bytes = tsv_bytes();
    assert_eq!(bytes.len(), 68);
    let digest = trusted_sha256(&bytes);
    assert_eq!(hex(&digest), TSV_SHA);
    let profile = tsv_profile();
    assert_eq!(profile.profile_preimage(), TSV_PREIMAGE);
    assert_eq!(profile.profile_digest(), trusted_sha256(TSV_PREIMAGE.as_bytes()));
    let reads = Arc::new(AtomicUsize::new(0));
    let max_buf = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(AtomicUsize::new(0));
    let chunked = Chunked {
        inner: Cursor::new(bytes.clone()),
        chunk: 1,
        reads: Arc::clone(&reads),
        max_buf: Arc::clone(&max_buf),
        bytes: Arc::clone(&got),
    };
    let (batches, exhausted, done) =
        pull(DelimitedReader::open(chunked, profile.clone(), digest, wide()).unwrap());
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(batch.raw_bytes, 31 + 37);
    assert_eq!(
        utf8_col(&batch.record_batch, 0),
        vec![Some("熊".to_owned()), Some("猫,鼠".to_owned())]
    );
    assert_eq!(
        utf8_col(&batch.record_batch, 1),
        vec![Some("a\tb".to_owned()), Some("line\r\n2".to_owned())]
    );
    assert_eq!(i64_col(&batch.record_batch, 2), vec![Some(i64::MIN), Some(i64::MAX)]);
    assert_eq!(batch.lineage[0].line, 1);
    assert_eq!(batch.lineage[0].byte_offset, 0);
    assert_eq!(field(&batch.lineage[0], "raw").byte_offset, 4);
    assert_eq!(batch.lineage[1].line, 2);
    assert_eq!(batch.lineage[1].byte_offset, 31);
    assert_eq!(field(&batch.lineage[1], "raw").line, 2);
    assert_eq!(field(&batch.lineage[1], "raw").byte_offset, 39);
    assert_eq!(field(&batch.lineage[1], "n").line, 3);
    assert_eq!(field(&batch.lineage[1], "n").byte_offset, 49);
    assert_eq!(exhausted.object_sha256, digest);
    assert_eq!(done.rows, 2);
    assert_eq!(done.object_sha256, digest);
    assert_not_admitted(&done);
    assert!(max_buf.load(Ordering::SeqCst) <= 256);
    assert_eq!(got.load(Ordering::SeqCst), bytes.len());
    assert!(reads.load(Ordering::SeqCst) > bytes.len());
    println!("tsv_sha256={}", hex(&digest));
    println!("tsv_complete_sha256={}", hex(&done.object_sha256));
    dump("positive.tsv", &bytes);
    dump("positive.tsv.sha256", format!("{}\n", hex(&digest)).as_bytes());
    dump("tsv-preimage.txt", TSV_PREIMAGE.as_bytes());
}

#[test]
fn profile_identity_has_no_budget_pii_or_inferred_dialect() {
    assert_eq!(DelimitedDialect::try_from_separator(b',').unwrap(), DelimitedDialect::Csv);
    assert_eq!(DelimitedDialect::try_from_separator(b'\t').unwrap(), DelimitedDialect::Tsv);
    for byte in [b';', b'|', b' ', b'"', b'\n', b'\r'] {
        assert_eq!(DelimitedDialect::try_from_separator(byte).unwrap_err().fact(), "ambiguous_dialect");
    }
    let csv = csv_profile();
    let shown = format!("{csv:?}");
    assert!(!shown.contains("pii") && !shown.contains("semantic"));
    let err = DelimitedProfile::try_new(
        "review-a199",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token(",".to_owned()),
        vec![col("h", DelimitedType::Utf8, true)],
    )
    .unwrap_err();
    assert_eq!(err.fact(), "null_token");
    let tsv_token = DelimitedProfile::try_new(
        "review-a199",
        DelimitedDialect::Tsv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token(",".to_owned()),
        vec![col("h", DelimitedType::Utf8, true)],
    );
    assert!(tsv_token.is_ok());
    let dup = DelimitedProfile::try_new(
        "review-a199",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("h", DelimitedType::Utf8, true), col("h", DelimitedType::Utf8, true)],
    )
    .unwrap_err();
    assert_eq!(dup.fact(), "duplicate_column");
}

#[test]
fn decimal_coefficients_and_refused_numbers_do_not_complete() {
    let decimal = DelimitedProfile::try_new(
        "review-a199-dec",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("amt", DelimitedType::Decimal { precision: 8, scale: 2 }, false)],
    )
    .unwrap();
    let bytes = b"1.20\n1.2\n-1.20\n";
    let digest = trusted_sha256(bytes);
    let (batches, _, done) =
        pull(DelimitedReader::open(Cursor::new(bytes.to_vec()), decimal.clone(), digest, wide()).unwrap());
    assert_eq!(expect_coeff("1.20", 2), 120);
    assert_eq!(expect_coeff("1.2", 2), 120);
    assert_eq!(expect_coeff("-1.20", 2), -120);
    assert_eq!(dec_col(&batches[0].record_batch, 0), vec![Some(120), Some(120), Some(-120)]);
    assert_eq!(
        batches[0].record_batch.schema().field(0).data_type(),
        &DataType::Decimal128(8, 2)
    );
    assert_not_admitted(&done);

    let negative = DelimitedProfile::try_new(
        "review-a199-neg",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("amt", DelimitedType::Decimal { precision: 8, scale: -2 }, false)],
    )
    .unwrap();
    let scaled = b"1200\n";
    let (batches, _, done) = pull(
        DelimitedReader::open(Cursor::new(scaled.to_vec()), negative.clone(), trusted_sha256(scaled), wide())
            .unwrap(),
    );
    assert_eq!(expect_coeff("1200", -2), 12);
    assert_eq!(dec_col(&batches[0].record_batch, 0), vec![Some(12)]);
    assert_eq!(batches[0].record_batch.schema().field(0).data_type(), &DataType::Decimal128(8, -2));
    assert_not_admitted(&done);
    assert_eq!(refuse(b"12\n", negative, wide()).fact(), "inexact_decimal");

    for (text, fact) in [
        ("+1.20\n", "not_decimal"),
        (" 1.20\n", "not_decimal"),
        ("1.\n", "not_decimal"),
        ("1e2\n", "not_decimal"),
        ("1.251\n", "inexact_decimal"),
        ("1000000.00\n", "decimal_precision"),
    ] {
        let err = refuse(text.as_bytes(), decimal.clone(), wide());
        assert_eq!(err.fact(), fact, "{text}");
        hidden(&err, text.trim());
    }

    let wide_dec = DelimitedProfile::try_new(
        "review-a199-d38",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("amt", DelimitedType::Decimal { precision: 38, scale: 0 }, false)],
    )
    .unwrap();
    let mut nines = "9".repeat(38);
    let mut expect: i128 = 0;
    for byte in nines.bytes() {
        expect = expect
            .checked_mul(10)
            .unwrap()
            .checked_add(i128::from(byte - b'0'))
            .unwrap();
    }
    nines.push('\n');
    let limit = budget(1, 1024, 128, 128, 1024, 4);
    let (batches, _, done) = pull(
        DelimitedReader::open(
            Cursor::new(nines.clone().into_bytes()),
            wide_dec.clone(),
            trusted_sha256(nines.as_bytes()),
            limit,
        )
        .unwrap(),
    );
    assert_eq!(dec_col(&batches[0].record_batch, 0), vec![Some(expect)]);
    assert_not_admitted(&done);
    let mut too_wide = "9".repeat(39);
    too_wide.push('\n');
    let err = refuse(too_wide.as_bytes(), wide_dec, limit);
    assert_eq!(err.fact(), "decimal_storage");
    hidden(&err, "999999");

    let integer = DelimitedProfile::try_new(
        "review-a199-int",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("n", DelimitedType::Int64, false)],
    )
    .unwrap();
    for (text, fact) in [
        ("+1\n", "not_int64"),
        ("01\n", "not_int64"),
        ("1.0\n", "not_int64"),
        ("9223372036854775808\n", "int64_overflow"),
        ("-9223372036854775809\n", "int64_overflow"),
    ] {
        assert_eq!(refuse(text.as_bytes(), integer.clone(), wide()).fact(), fact, "{text}");
    }
}

#[test]
fn malformed_width_budget_and_hash_do_not_complete() {
    let text = one_text(HeaderPolicy::Absent, BomPolicy::Refuse, NullPolicy::Empty);
    let err = refuse(b"\"SHOULD_NOT_LEAK_a199", text.clone(), wide());
    assert_eq!(err.kind(), DelimitedErrorKind::Protocol);
    assert_eq!(err.fact(), "malformed_quote");
    hidden(&err, "SHOULD_NOT_LEAK_a199");
    let err = refuse(b"\"a\"SHOULD_NOT_LEAK_a199\n", text.clone(), wide());
    assert_eq!(err.fact(), "malformed_quote");
    hidden(&err, "SHOULD_NOT_LEAK_a199");
    assert_eq!(refuse(b"\xff\n", text.clone(), wide()).fact(), "utf8");
    assert_eq!(refuse(b"a\x00b\n", text.clone(), wide()).fact(), "control_char");
    assert_eq!(refuse(b"hello\r", text.clone(), wide()).fact(), "bare_cr");
    let kept = b"\"a\rb\"\n";
    let (batches, _, done) =
        pull(DelimitedReader::open(Cursor::new(kept.to_vec()), text.clone(), trusted_sha256(kept), wide()).unwrap());
    assert_eq!(utf8_col(&batches[0].record_batch, 0), vec![Some("a\rb".to_owned())]);
    assert_not_admitted(&done);
    let spaces = b" a \n";
    let (batches, _, _) = pull(
        DelimitedReader::open(Cursor::new(spaces.to_vec()), text.clone(), trusted_sha256(spaces), wide()).unwrap(),
    );
    assert_eq!(utf8_col(&batches[0].record_batch, 0), vec![Some(" a ".to_owned())]);

    let err = refuse(b"a,b\n", text.clone(), wide());
    assert_eq!(err.fact(), "width_mismatch");
    let exact = one_text(HeaderPolicy::Exact, BomPolicy::Refuse, NullPolicy::Empty);
    assert_eq!(refuse(b"wrong\n", exact.clone(), wide()).fact(), "header_mismatch");
    let mut reader = DelimitedReader::open(Cursor::new(Vec::<u8>::new()), exact, [0x11; 32], wide()).unwrap();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.fact(), "header_required");
    assert_eq!(reader.finish().unwrap_err().fact(), "header_required");

    let absent = one_text(HeaderPolicy::Absent, BomPolicy::Refuse, NullPolicy::Empty);
    let mut reader = DelimitedReader::open(Cursor::new(Vec::<u8>::new()), absent.clone(), [0x22; 32], wide()).unwrap();
    assert_eq!(reader.next_batch().unwrap_err().fact(), "hash_mismatch");
    assert_eq!(reader.finish().unwrap_err().fact(), "hash_mismatch");
    let empty_hash = trusted_sha256(b"");
    let (_, exhausted, done) =
        pull(DelimitedReader::open(Cursor::new(Vec::<u8>::new()), absent, empty_hash, wide()).unwrap());
    assert_eq!(exhausted.rows, 0);
    assert_eq!(done.rows, 0);
    assert_eq!(done.object_sha256, empty_hash);
    assert_not_admitted(&done);

    let bom = one_text(HeaderPolicy::Absent, BomPolicy::Require, NullPolicy::Empty);
    assert_eq!(refuse(b"a\n", bom.clone(), wide()).fact(), "bom_required");
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice("熊\n".as_bytes());
    let (batches, _, done) = pull(
        DelimitedReader::open(Cursor::new(with_bom.clone()), bom, trusted_sha256(&with_bom), wide()).unwrap(),
    );
    assert_eq!(utf8_col(&batches[0].record_batch, 0), vec![Some("熊".to_owned())]);
    assert_eq!(batches[0].lineage[0].byte_offset, 3);
    assert_eq!(batches[0].lineage[0].line, 1);
    assert_not_admitted(&done);
    let refused = one_text(HeaderPolicy::Absent, BomPolicy::Refuse, NullPolicy::Empty);
    assert_eq!(refuse(&with_bom, refused, wide()).fact(), "bom_refused");

    let later = one_text(HeaderPolicy::Absent, BomPolicy::Refuse, NullPolicy::Empty);
    let mut mid = Vec::new();
    mid.extend_from_slice("a".as_bytes());
    mid.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    mid.extend_from_slice("b\n".as_bytes());
    let (batches, _, _) = pull(
        DelimitedReader::open(Cursor::new(mid.clone()), later, trusted_sha256(&mid), wide()).unwrap(),
    );
    assert_eq!(utf8_col(&batches[0].record_batch, 0), vec![Some("a\u{feff}b".to_owned())]);

    let err = refuse(b"\"abcdef\"\n", text.clone(), budget(1, 1024, 8, 8, 64, 10));
    assert_eq!(err.fact(), "max_record");
    let err = refuse(b"abcdef\n", text.clone(), budget(1, 1024, 32, 4, 64, 10));
    assert_eq!(err.fact(), "max_field");
    hidden(&err, "abcdef");
    // max_total must hold one record. The window is 4 bytes and the file is 5.
    let err = refuse(b"ab\nXY", text.clone(), budget(4, 1024, 4, 4, 4, 10));
    assert_eq!(err.fact(), "max_total");
    let err = refuse(b"aa\n", text.clone(), budget(8, 1, 64, 64, 1024, 10));
    assert_eq!(err.fact(), "batch_bytes");

    let split = budget(8, 3, 64, 64, 1024, 10);
    let (batches, _, done) = pull(
        DelimitedReader::open(Cursor::new(b"aa\nb\n".to_vec()), text.clone(), trusted_sha256(b"aa\nb\n"), split)
            .unwrap(),
    );
    assert_eq!(batches.len(), 2);
    assert!(batches[0].raw_bytes <= 3);
    assert!(batches[1].raw_bytes <= 3);
    assert_eq!(utf8_col(&batches[0].record_batch, 0), vec![Some("aa".to_owned())]);
    assert_eq!(utf8_col(&batches[1].record_batch, 0), vec![Some("b".to_owned())]);
    assert_eq!(batches[1].lineage[0].row_index, 1);
    assert_not_admitted(&done);

    let rows = budget(8, 1024, 64, 64, 1024, 1);
    let mut reader =
        DelimitedReader::open(Cursor::new(b"a\nb\n".to_vec()), text.clone(), trusted_sha256(b"a\nb\n"), rows).unwrap();
    assert!(matches!(reader.next_batch().unwrap(), DelimitedItem::Batch(_)));
    assert_eq!(reader.next_batch().unwrap_err().fact(), "max_rows");
    assert_eq!(reader.finish().unwrap_err().fact(), "max_rows");

    let narrow = DelimitedBudget::try_new(
        4,
        1024,
        64,
        64,
        1024,
        4,
        1,
        1,
        Instant::now() + Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(refuse(b"a,b\n", text.clone(), narrow).fact(), "max_fields");

    let file = b"aa\nbb\n";
    let real = trusted_sha256(file);
    let mut wrong = real;
    wrong[0] ^= 0xff;
    let mut reader = DelimitedReader::open(Cursor::new(file.to_vec()), text.clone(), wrong, wide()).unwrap();
    let DelimitedItem::Batch(batch) = reader.next_batch().unwrap() else {
        panic!("hash mismatch returned no batch");
    };
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some("aa".to_owned()), Some("bb".to_owned())]);
    assert!(!batch.header.admits_import && !batch.header.published);
    assert_eq!(reader.next_batch().unwrap_err().fact(), "hash_mismatch");
    assert_eq!(reader.finish().unwrap_err().fact(), "hash_mismatch");

    let mut reader = DelimitedReader::open(Cursor::new(b"a\n".to_vec()), text, trusted_sha256(b"a"), wide()).unwrap();
    let DelimitedItem::Batch(batch) = reader.next_batch().unwrap() else {
        panic!("prefix hash dropped the pending row");
    };
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some("a".to_owned())]);
    assert!(!batch.header.admits_import);
    assert_eq!(reader.next_batch().unwrap_err().fact(), "hash_mismatch");
    assert_eq!(reader.finish().unwrap_err().fact(), "hash_mismatch");
}

#[test]
fn early_stop_cancel_and_blocking_read_do_not_complete() {
    let profile = one_text(HeaderPolicy::Absent, BomPolicy::Refuse, NullPolicy::Empty);
    let digest = trusted_sha256(b"a\nb\n");
    let reader = DelimitedReader::open(Cursor::new(b"a\nb\n".to_vec()), profile.clone(), digest, wide()).unwrap();
    assert_eq!(reader.finish().unwrap_err().fact(), "not_exhausted");
    let mut reader = DelimitedReader::open(Cursor::new(b"a\nb\n".to_vec()), profile.clone(), digest, budget(1, 1024, 64, 64, 1024, 10)).unwrap();
    assert!(matches!(reader.next_batch().unwrap(), DelimitedItem::Batch(_)));
    assert_eq!(reader.finish().unwrap_err().fact(), "not_exhausted");

    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Counted { data: b"a\n".to_vec(), pos: 0, reads: Arc::clone(&reads) };
    let mut reader = DelimitedReader::open(counted, profile.clone(), trusted_sha256(b"a\n"), wide()).unwrap();
    reader.cancel_handle().request();
    assert_eq!(reader.next_batch().unwrap_err().fact(), "cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(reader.finish().unwrap_err().fact(), "cancelled");

    let due = DelimitedBudget::try_new(1, 16, 8, 8, 16, 1, 1, 1, Instant::now()).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let err = DelimitedReader::open(
        Counted { data: b"a\n".to_vec(), pos: 0, reads: Arc::clone(&reads) },
        profile.clone(),
        trusted_sha256(b"a\n"),
        due,
    )
    .unwrap_err();
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let mut reader = DelimitedReader::open(
        Hold {
            data: b"a\n".to_vec(),
            pos: 0,
            blocked: false,
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            reads: Arc::clone(&reads),
        },
        profile.clone(),
        trusted_sha256(b"a\n"),
        wide(),
    )
    .unwrap();
    let cancel = reader.cancel_handle();
    let finished = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&finished);
    thread::scope(|scope| {
        let handle = scope.spawn(move || {
            let pulled = match reader.next_batch() {
                Ok(_) => "returned".to_owned(),
                Err(err) => err.fact().to_owned(),
            };
            let ended = match reader.finish() {
                Ok(_) => "complete".to_owned(),
                Err(err) => err.fact().to_owned(),
            };
            flag.store(true, Ordering::SeqCst);
            (pulled, ended)
        });
        let start = Instant::now();
        while !entered.load(Ordering::SeqCst) {
            assert!(start.elapsed() < Duration::from_secs(5), "blocked read did not start");
            thread::sleep(Duration::from_millis(1));
        }
        cancel.request();
        thread::sleep(Duration::from_millis(80));
        assert!(!finished.load(Ordering::SeqCst), "pull returned while read was blocked");
        release.store(true, Ordering::SeqCst);
        let (pulled, ended) = handle.join().unwrap();
        assert_eq!(pulled, "cancelled");
        assert_eq!(ended, "cancelled");
    });
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    println!("blocking_cancel reads=1");

    let entered = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_millis(800);
    let limit = DelimitedBudget::try_new(1, 64, 16, 16, 64, 4, 1, 1, deadline).unwrap();
    let mut reader = DelimitedReader::open(
        SleepOnce {
            data: b"a\n".to_vec(),
            pos: 0,
            slept: false,
            millis: 2_000,
            entered: Arc::clone(&entered),
        },
        profile.clone(),
        trusted_sha256(b"a\n"),
        limit,
    )
    .unwrap();
    let started = Instant::now();
    let err = reader.next_batch().unwrap_err();
    let elapsed = started.elapsed();
    assert!(entered.load(Ordering::SeqCst));
    assert!(elapsed >= Duration::from_millis(1_800), "deadline interrupted the read: {elapsed:?}");
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reader.finish().unwrap_err().fact(), "deadline");
    println!("blocking_deadline_ms={}", elapsed.as_millis());

    let flag = Arc::new(AtomicBool::new(false));
    let reader = DelimitedReader::open(
        FlagRead { flag: Arc::clone(&flag), data: "熊\n".as_bytes().to_vec(), pos: 0 },
        profile.clone(),
        trusted_sha256("熊\n".as_bytes()),
        wide(),
    )
    .unwrap();
    let shown = format!("{reader:?}");
    assert!(!shown.contains('熊'));
    drop(reader);
    assert!(flag.load(Ordering::SeqCst));

    let mut reader = DelimitedReader::open(SecretRead, profile, trusted_sha256(b""), wide()).unwrap();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.kind(), DelimitedErrorKind::Io);
    assert_eq!(err.fact(), "read");
    hidden(&err, "SECRET_OS_a199");
    hidden(&reader.finish().unwrap_err(), "SECRET_OS_a199");
}

struct Counted {
    data: Vec<u8>,
    pos: usize,
    reads: Arc<AtomicUsize>,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.pos >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

struct Hold {
    data: Vec<u8>,
    pos: usize,
    blocked: bool,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}

impl Read for Hold {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if !self.blocked {
            self.blocked = true;
            self.entered.store(true, Ordering::SeqCst);
            while !self.release.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(5));
            }
        }
        if self.pos >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

struct SleepOnce {
    data: Vec<u8>,
    pos: usize,
    slept: bool,
    millis: u64,
    entered: Arc<AtomicBool>,
}

impl Read for SleepOnce {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.slept {
            self.slept = true;
            self.entered.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(self.millis));
        }
        if self.pos >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

struct FlagRead {
    flag: Arc<AtomicBool>,
    data: Vec<u8>,
    pos: usize,
}

impl Drop for FlagRead {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

impl Read for FlagRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

struct SecretRead;

impl Read for SecretRead {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "SECRET_OS_a199",
        ))
    }
}
