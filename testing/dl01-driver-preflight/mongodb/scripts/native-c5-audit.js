const fs=require('fs');
if(!db.getSiblingDB('admin').auth('oo_dl01w1_41_audit',fs.readFileSync('/run/secrets/audit','utf8').trim()))quit(2);
const entries=db.getSiblingDB('oo_w141').system.profile.find({user:'oo_dl01w1_41_reader@admin'}).toArray();
const forbidden=['insert','update','delete','findAndModify','create','drop','createIndexes','collMod','dropDatabase'];
const rows=entries.map(e=>({op:e.op,command:Object.keys(e.command||{})[0],comment:(e.command||{}).comment||null}));
const writes=rows.filter(e=>forbidden.includes(e.command)||['insert','update','remove'].includes(e.op));
print(JSON.stringify({protocol:'oo.mongodb.native-c5-audit/v1',reader_events:rows.length,reader_dml_ddl_attempts:writes.length,rows,scope:'new isolated fixture profile since creation'}));
if(!rows.length||writes.length)quit(3);
