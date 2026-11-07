// Dependency-free Chrome DevTools pipe test; no listener or browser profile reuse.
import {spawn} from 'node:child_process';
import {readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';
const [rootArg, chrome] = process.argv.slice(2);
assert(rootArg && chrome, 'usage: node ontology_browser.mjs EVIDENCE_DIR CHROME');
const root = resolve(rootArg);
const html = await readFile(join(root, 'atlas.html'), 'utf8');
for (const pattern of [/https?:\/\//, /\bfetch\s*\(/, /\bsrc\s*=/, /XMLHttpRequest/, /WebSocket/]) assert(!pattern.test(html), String(pattern));
for (const width of [1280,390]) {
  const child = spawn(chrome, ['--headless=new', '--remote-debugging-pipe', '--no-first-run', '--disable-background-networking', '--disable-component-update', '--disable-sync', '--user-data-dir='+join(root,'cdp-'+width)], {stdio:['ignore','ignore','pipe','pipe','pipe']});
  let sequence=0, buffer='', errors='';
  const pending=new Map();
  child.stderr.on('data', d=>errors+=d);
  child.stdio[4].on('data', data=>{
    buffer+=data.toString();
    let end;
    while ((end=buffer.indexOf('\0'))>=0) {
      const message=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);
      if(pending.has(message.id)){const {resolve,reject,timer}=pending.get(message.id);pending.delete(message.id);clearTimeout(timer);message.error?reject(new Error(JSON.stringify(message.error))):resolve(message.result);}
    }
  });
  const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{
    const id=++sequence;
    const timer=setTimeout(()=>{pending.delete(id);reject(new Error('CDP timeout: '+method));},15000);
    pending.set(id,{resolve,reject,timer});child.stdio[3].write(JSON.stringify({id,method,params,...(sessionId?{sessionId}:{})})+'\0');
  });
  try {
    const {targetId}=await send('Target.createTarget',{url:'about:blank'});
    const {sessionId}=await send('Target.attachToTarget',{targetId,flatten:true});
    const call=(method,params)=>send(method,params,sessionId);
    await call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:width<700});
    await call('Page.navigate',{url:pathToFileURL(join(root,'atlas.html')).href});
    const ready=await call('Runtime.evaluate',{expression:`new Promise((resolve,reject)=>{const deadline=Date.now()+5000;const poll=()=>{if(document.documentElement.dataset.renderMs)resolve(true);else if(Date.now()>deadline)reject(new Error('render timeout'));else setTimeout(poll,25);};poll();})`,awaitPromise:true,returnByValue:true});
    assert(!ready.exceptionDetails, JSON.stringify(ready));
    const report=await call('Runtime.evaluate',{expression:`(()=>{const results=[];

  function check(name, condition) { results.push({name, pass: Boolean(condition)}); }
  check('1200_entities', pack.entities.length === 1200);
  check('initial_virtualized', document.querySelectorAll('button.entity').length < 20);
  $('list').scrollTop = 1199*44; render();
  check('last_entity_reachable', [...document.querySelectorAll('button.entity')].some(b=>b.title==='e1199'));
  document.querySelector('button.entity:last-child').click();
  check('entity_detail', $('title').textContent==='Entity 1199');
  check('columns_and_relations', $('detail').textContent.includes('"name": "id"') && $('detail').textContent.includes('link'));
  check('metric_detail', $('detail').textContent.includes('metrics/count.md'));
  $('search').value='Entity 1199'; $('search').dispatchEvent(new Event('input'));
  check('search', filtered.length === 1);
  $('search').value=''; $('community').value='c0'; $('community').dispatchEvent(new Event('change'));
  check('community_filter', filtered.length === 50);
  $('domain').value='db'; $('domain').dispatchEvent(new Event('change'));
  check('domain_filter', filtered.every(i=>pack.entities[i].domain==='db'));
  check('layout_once', document.documentElement.dataset.layoutPasses === '1');
  check('render_under_2s', Number(document.documentElement.dataset.renderMs) < 2000);
  check('no_subresource_requests', performance.getEntriesByType('resource').length === 0);
  check('no_script_error', !document.documentElement.dataset.probeError);

return {render_ms:Number(document.documentElement.dataset.renderMs),results};})()`,returnByValue:true});
    assert(!report.exceptionDetails, JSON.stringify(report));
    const value=report.result.value;
    await writeFile(join(root,`browser-${width}.json`),JSON.stringify(value,null,2)+'\n');
    console.log(JSON.stringify({width,...value}));
    assert(value.results.every(r=>r.pass));
    const screenshot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(join(root,`browser-${width}.png`),Buffer.from(screenshot.data,'base64'));
  } finally {
    await writeFile(join(root,`browser-cdp-${width}.stderr`),errors);
    try { await send('Browser.close'); } finally {child.kill();}
  }
}
