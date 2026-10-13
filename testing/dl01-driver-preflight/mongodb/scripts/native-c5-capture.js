// Synthetic owned source: full typed EJSON read, no source writes.
const fs = require('fs');
const admin = db.getSiblingDB('admin');
if (!admin.auth('oo_dl01w1_41_reader', fs.readFileSync('/run/secrets/reader','utf8').trim())) quit(2);
const source = db.getSiblingDB('oo_w141');
const who = admin.runCommand({connectionStatus:1});
const hello = admin.runCommand({hello:1});
const approved=new Set(['_id','code','flag','units','price','created','uid','blob','weight','warehouse','whCode','lane','memo','marks','pack']);
// Inspect field names/types without exporting unknown field values. Full raw
// BSON reads are limited to documents whose complete field set is approved.
const census=source.shipments.aggregate([{$project:{pairs:{$map:{input:{$objectToArray:'$$ROOT'},as:'p',in:{name:'$$p.k',type:{$type:'$$p.v'}}}}}}]).toArray();
const refused=census.filter(d=>d.pairs.some(p=>!approved.has(p.name))).map(d=>({key:d._id.toHexString(),fields:d.pairs.filter(p=>!approved.has(p.name)),loaded:false,raw_doc_authorized:false}));
const keys=census.filter(d=>d.pairs.every(p=>approved.has(p.name))).map(d=>d._id);
const cursor = source.shipments.find({_id:{$in:keys},$expr:{$eq:[{$size:{$setDifference:[{$map:{input:{$objectToArray:'$$ROOT'},as:'p',in:'$$p.k'}},Array.from(approved)]}},0]}}).sort({_id:1}).comment('oo-dl01w1-41:native-c5:reader:capture');
const docs = cursor.toArray();
if (census.length > 16) throw Error('budget.rows');
const rows = docs.map(d => ({key:d._id.toHexString(), document:EJSON.serialize(d,{relaxed:false})}));
print(JSON.stringify({protocol:'oo.mongodb.native-c5-capture/v1',capture:'C5',consistency:'B',delete_detection:'reconcile',quality:['DELETES_LAGGED'],snapshot_token:null,topology:hello.setName?'replica':'standalone',source_rows:census.length,scope_refused:refused,rows,users:who.authInfo.authenticatedUsers}));
