//! Minimal native parse/deparse smoke. Not an admission decision.
fn main() {
    let sql = "SELECT 1";
    let parsed = pg_query::parse(sql).expect("pg_query parse failed");
    let version = parsed.protobuf.version;
    let deparsed = parsed.deparse().expect("pg_query deparse failed");
    let again = pg_query::parse(&deparsed).expect("reparse failed");
    println!("smoke_ok=1");
    println!("parser_tree_version={version}");
    println!("expected_pg_version_num=170007");
    println!("deparsed={deparsed}");
    println!("reparse_version={}", again.protobuf.version);
    println!("stmt_count={}", parsed.protobuf.stmts.len());
    if version != 170007 || again.protobuf.version != 170007 {
        eprintln!("parser version mismatch");
        std::process::exit(2);
    }
    if parsed.protobuf.stmts.len() != 1 {
        eprintln!("expected one statement");
        std::process::exit(3);
    }
}
