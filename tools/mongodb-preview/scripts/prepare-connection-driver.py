#!/usr/bin/env python3
"""Prepare a tools-owned endpoint overlay of the frozen read-only BSON driver."""
import argparse, hashlib, json, pathlib, shutil
ROOT=pathlib.Path(__file__).resolve().parents[3]
def main():
 p=argparse.ArgumentParser();p.add_argument('--out',required=True);a=p.parse_args()
 out=pathlib.Path(a.out);out.mkdir(parents=True,exist_ok=False)
 base=ROOT/'testing/dl01-driver-preflight/mongodb'
 shutil.copytree(base/'probe',out/'probe');shutil.copy(base/'Cargo.lock',out/'Cargo.lock')
 manifest=(base/'Cargo.toml').read_text().replace('members = ["smoke", "probe", "decode"]','members = ["probe"]')
 (out/'Cargo.toml').write_text(manifest)
 path=out/'probe/src/main.rs';text=path.read_text();text=text.replace('mod customer;','mod customer;\nmod connection;')
 old='''    options.credential = Some(
        Credential::builder()
            .username(Some("oo_dl01w1_41_reader".to_string()))'''
 new='''    let connection = connection::load()?;
    options.hosts = vec![mongodb::options::ServerAddress::Tcp { host: connection.host, port: Some(connection.port) }];
    options.credential = Some(
        Credential::builder()
            .username(Some(connection.user))'''
 assert old in text;text=text.replace(old,new).replace('.source(Some("admin".to_string()))','.source(Some(connection.auth_database))')
 path.write_text(text)
 shutil.copy(ROOT/'tools/mongodb-preview/connection.rs',out/'probe/src/connection.rs')
 files={str(f.relative_to(out)):hashlib.sha256(f.read_bytes()).hexdigest() for f in out.rglob('*') if f.is_file()}
 (out/'source-manifest.json').write_text(json.dumps({'protocol':'oo.mongodb.connection-driver-source/v1','files':files},sort_keys=True)+'\n')
if __name__=='__main__':main()
