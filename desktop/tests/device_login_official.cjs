// Private recovered inputs are optional research material, never public CI input.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const assert = require('node:assert/strict'), crypto = require('node:crypto');
const parser = require('../../frontend/node_modules/@babel/parser');
const root = path.resolve(process.argv[2] || 'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names = ['hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js', 'hybrid_app.chunk.1482700000000.js/15.js', 'hybrid_app.chunk.1482700000000.js/48.js'];
const source = names.map(n => fs.readFileSync(path.join(root,n),'utf8'));
const ast = source.map(s => parser.parse(s));
function walk(node, visit) {
  if (!node || typeof node !== 'object') return;
  visit(node);
  for (const v of Object.values(node)) Array.isArray(v) ? v.forEach(n => walk(n,visit)) : walk(v,visit);
}
const code = (n,i=0) => source[i].slice(n.start,n.end);
function method(name) {
  const found=[];
  walk(ast[0],n=>{
    if (n.type!=='ObjectExpression' || !n.properties.some(p=>p.key?.name==='key' && p.value?.value===name)) return;
    const fn=n.properties.find(p=>p.key?.name==='value')?.value;
    if (fn && code(fn).includes('.fetch(')) found.push(fn);
  });
  assert.equal(found.length,1,name);
  return found[0];
}
let serialize;
walk(ast[2],n=>{if(n.type==='FunctionDeclaration' && n.id.name==='f') serialize=n;});
assert(serialize);
// Execute original M/48 form serializer, including undefined/null string values.
const form=vm.runInNewContext('('+code(serialize,2)+')',{
  i:{a:v=>v!==null && typeof v==='object' && !Array.isArray(v)},
  s:encodeURIComponent,u:v=>'['+v.map(encodeURIComponent).join(',')+']',
  d:v=>encodeURIComponent(JSON.stringify(v)),
});
function fields(node, env, receiver={}) {
  const value=vm.runInNewContext('(function(){return ('+code(node)+')}).call(receiver)',{...env,receiver});
  value.fromPage='JSK';
  return Object.fromEntries(new URLSearchParams(form(value)));
}
const args = {
  startDeviceApply:{e:'Synthetic PC',n:undefined}, startScanApply:{e:'Synthetic PC',n:undefined},
  checkDeviceStatus:{}, checkScanApplyStatus:{},
  checkDeviceCaptcha:{e:'fixture-device-token',n:'123456',r:undefined},
  cancelDeviceApply:{e:'fixture-device-token',n:undefined}, cancelScanApply:{e:'fixture-device-token',n:undefined},
};
const vectors={};
for(const [name,env] of Object.entries(args)) {
  let object;
  walk(method(name),n=>{if(n.type==='CallExpression' && n.callee.property?.name==='fetch' && n.arguments[1]?.type==='ObjectExpression') object=n.arguments[1];});
  assert(object,name);
  vectors[name]=fields(object,env,{currentPollingToken:'fixture-device-token',currentScene:name==='checkDeviceStatus'?null:undefined});
}
const legacy={};
walk(ast[1],n=>{
  if(n.type!=='CallExpression' || n.callee.type!=='CallExpression') return;
  const config=n.callee.arguments[0];
  if(config?.type!=='ObjectExpression') return;
  const url=config.properties.find(p=>p.key?.name==='url')?.value?.value;
  if(!url?.startsWith('/api/login/origin-device/')) return;
  legacy[url]=vm.runInNewContext('('+code(n.arguments[0],1)+')',{e:url.endsWith('/start')?'Synthetic PC':'fixture-device-token',t:'123456'});
});
assert.equal(Object.keys(legacy).length,7);
const stopBehavior={};
for(const [name,member] of [['stopScanVerify','scanVerifyHandler'],['stopDeviceVerify','deviceVerifyHandler']]) {
  const found=[];
  walk(ast[0],n=>{
    if(n.type!=='ObjectExpression'||!n.properties.some(p=>p.key?.name==='key'&&p.value?.value===name))return;
    const fn=n.properties.find(p=>p.key?.name==='value')?.value;
    if(fn && code(fn).includes(member+'.stopPolling'))found.push(fn);
  });
  assert.equal(found.length,1,name);let stops=0;
  vm.runInNewContext('('+code(found[0])+')').call({[member]:{stopPolling(){stops++}}});
  assert.equal(stops,1);stopBehavior[name]={pollingStops:stops,networkRequests:0};
}
console.log(JSON.stringify({sources:Object.fromEntries(names.map((n,i)=>[n,crypto.createHash('sha256').update(source[i]).digest('hex')])),jsk:vectors,legacy,stopBehavior},null,2));
