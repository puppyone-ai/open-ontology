# Erratum — embedded Unicode 16 tables

This note corrects the provenance of `crates/oo-builder/src/nfc_tables.rs` as that file existed before it was removed from the product. The removal does not rewrite history. The file remains in git history. Review a071 (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a071/SP07/structure-draft-review/`) is unchanged, including `review.md` and `findings.json`.

The file header said:

> Generated from Unicode 16.0.0. Hangul syllables U+AC00..=U+D7A3 are composed in code and are not in these tables.

No generator script was in the tree. That sentence is not a supported provenance claim. a071 reconstructed the tables from the Unicode 16.0.0 UCD and found decomposition, combining class, and composition pairs equal, with nothing missing and nothing extra. The bytes were a copy of Unicode data. Neither the file nor the repository `NOTICE` at that commit carried the Unicode copyright and permission notice. a071 records `nfc_tables.rs` SHA-256 `794de9ec4ad9048a188b6174d467b044ac71d50a9e8f099c66ecd41bf272b338` and `NOTICE` SHA-256 `463cbfaba4a8bc4e494e2daf307334b7cee314f10a137e06b6bbe43c502026fc`.

Deleting the module does not make that header accurate, and it does not make the old `NOTICE` a Unicode notice. Drafts compiled with `nfc.rs` from that history used the hand-written composer, which disagrees with Unicode NFC on equal-class marks and on 34 Unicode 17 combining classes. Those drafts are not rehashed here.

The replacement is `unicode-normalization` 0.1.25 (Unicode 17.0.0), the same version the PostgreSQL connector pins. The crate `COPYRIGHT` grants MIT OR Apache-2.0 and does not include the Unicode data notice. That notice is `tests/unicode/LICENSE.txt`, `tests/unicode/PROVENANCE.md`, `NOTICE.md` in this crate, and the repository `NOTICE`.
