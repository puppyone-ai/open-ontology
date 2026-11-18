//! The engine learns the current parser version from the sidecar's `GET /v1/info` (D23): a result of another
//! version is not reused and its file becomes `outdated`. The sidecar answers `parser_id` and `parser_version` at the top
//! level; a parse result header nests them under `parser`. Both are read.

use oo_docs::jobs::stages::parser_of_info;
use serde_json::json;

#[test]
fn the_info_of_a_sidecar_names_its_parser() {
    let info = json!({"protocol": "oo.doc-parser/v1", "parser_id": "media", "parser_version": "3", "adapter_version": "1"});
    assert_eq!(parser_of_info(&info), Some(("media".into(), "3".into())));
}

#[test]
fn a_result_header_is_read_too() {
    let r = json!({"parser": {"id": "docling", "version": "2.1", "adapter_version": "1"}});
    assert_eq!(parser_of_info(&r), Some(("docling".into(), "2.1".into())));
    assert_eq!(parser_of_info(&json!({"protocol": "x"})), None);
}
