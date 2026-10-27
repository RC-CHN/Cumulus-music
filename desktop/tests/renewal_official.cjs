// Execute the recovered PC bridge and generic login provider without network.
const fs=require('node:fs'), vm=require('node:vm'), assert=require('node:assert/strict'), crypto=require('node:crypto');
const parser=require('../../frontend/node_modules/@babel/parser');
function traverse(node, visitors) {
 if (!node || typeof node !== 'object') return;
 visitors[node.type]?.({node});
 for (const value of Object.values(node)) {
  if (Array.isArray(value)) value.forEach(child=>traverse(child,visitors));
  else if(value && typeof value === 'object') traverse(value,visitors);
 }
}
const base='references/netease-3.1.42.205594-analysis/recovered-js/modules/';
const paths=[base+'hybrid_app.chunk.1482700000000.js/290.js',base+'hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js'];
const sources=paths.map(p=>fs.readFileSync(p,'utf8'));
let bridge, gate, provider;
traverse(parser.parse(sources[0]), { ObjectProperty(p) {
  if(p.node.key?.name==='ydDeviceToken')bridge=sources[0].slice(p.node.value.start,p.node.value.end);
  if(p.node.key?.name==='jskConfig')gate=sources[0].slice(p.node.value.start,p.node.value.end);
}});
traverse(parser.parse(sources[1]), { ObjectExpression(p) {
  if(p.node.properties.some(v=>v.key?.name==='key'&&v.value?.value==='getYdDeviceToken')) {const n=p.node.properties.find(v=>v.key?.name==='value').value;provider=sources[1].slice(n.start,n.end);}
}});
assert(bridge&&gate&&provider);
(async()=>{
 const call=vm.runInNewContext('('+bridge+')');
 let response; await call({},r=>response=r); assert.equal(response.code,200); assert.equal(response.token,undefined);
 let flags; await vm.runInNewContext('('+gate+')')({},r=>flags=r); assert.equal(flags.enableTokenJsk,true);
 async function get(eventToken) {
  let listener,timeout,delay,removed=0,settled=false;
  const fn=vm.runInNewContext('('+provider+')', {
   hr:{ getAppInfo:async()=>({platform:'pc'}),getYdDeviceToken:()=>call({},()=>{}),on:(_,name,f)=>{assert.equal(name,'com.netease.cloudmusic.web.action.event.publish.SEND_YD_TOKEN');listener=f},off:()=>removed++ },
   Ve:{log(){}},setTimeout:(f,ms)=>{timeout=f;delay=ms}
  });
  const result=fn().then(v=>{settled=true;return v}); await Promise.resolve(); await Promise.resolve();
  assert.equal(settled,false);assert.equal(delay,5000);
  if(eventToken!==undefined)listener({token:eventToken});else timeout();
  const value=await result;timeout(); assert.equal(removed,1);return value;
 }
 assert.equal(await get(), ''); assert.equal(await get('actual-event-token'),'actual-event-token');
 console.log(JSON.stringify({ok:true,desktopBridgeNoToken:true,desktopJskEnabled:true,providerTimeoutMs:5000,emptyTimeoutForwarded:true,eventTokenPreserved:true,sourceHashes:Object.fromEntries(paths.map((p,i)=>[p,crypto.createHash('sha256').update(sources[i]).digest('hex')]))},null,2));
})().catch(e=>{console.error(e);process.exitCode=1});
