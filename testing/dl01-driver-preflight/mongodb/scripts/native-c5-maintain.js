// DBA-only mutations of new isolated synthetic fixture.
const fs=require('fs');
if(!db.getSiblingDB('admin').auth('oo_dl01w1_41_dba',fs.readFileSync('/run/secrets/dba','utf8').trim()))quit(2);
const source=db.getSiblingDB('oo_w141');
const step=Number(process.env.C5_STEP);
const result=source.runCommand({update:'shipments',updates:[{q:{_id:ObjectId('c141d04141eef04100000001')},u:{$set:{memo:'native-cycle-'+step,weight:42.25+step}}}],comment:'oo-dl01w1-41:native-c5:dba:maintain'});
if(result.ok!==1||result.n!==1)throw Error('fixture update failed');
print(JSON.stringify({step,matched:result.n,modified:result.nModified}));
