// Compare the bridge options and result handoff to the recovered official M256.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const officialPath = 'references/netease-3.1.42.205594-analysis/recovered-js/modules/hybrid_app.chunk.1482700000000.js/256.js';
function environment() {
  const nodes = new Map();
  const scripts = [];
  const context = {
    document: {
      body: {dataset: {}, appendChild: s => scripts.push(s)},
      head: {appendChild: s => scripts.push(s)},
      createElement: () => ({}), querySelectorAll: () => [],
      getElementById: id => { if(!nodes.has(id))nodes.set(id, {}); return nodes.get(id); },
    },
    window: {}, location: {href: ''}, setTimeout, clearTimeout,
  };
  let options;
  context.window.initNECaptcha = (opts, done) => { options = opts; done({verify(){},destroy(){}}); };
  return {context, scripts, options:()=>options};
}
const original = environment(), exportsObject={}, dispatched=[];
const dependencies = {
  2: {b:{error(){}}}, 52:{g:e=>dispatched.push(e)}, 34:{p:()=>false},
  218:{d:()=>false}, 8:{a:{inited:false}},
};
const load = id => { assert(id in dependencies); return dependencies[id]; };
load.d=(object, key, get)=>Object.defineProperty(object,key,{get});
vm.runInNewContext(fs.readFileSync(officialPath,'utf8'), original.context)({},exportsObject,load);
exportsObject.b(); original.scripts[0].onload();
const ours=environment();
vm.runInNewContext(fs.readFileSync('desktop/src/captcha/page.js','utf8'),ours.context);
assert.equal(ours.scripts[0].src,original.scripts[0].src);
ours.scripts[0].onload();
for(const field of ['captchaId','width','mode','apiVersion'])assert.equal(ours.options()[field],original.options()[field]);
assert.equal(JSON.stringify(ours.options().popupStyles),JSON.stringify(original.options().popupStyles));
assert.equal(ours.options().element,'#captcha'); // Explicit isolated-window mounting difference.
const proof='synthetic +/?&中文';
original.options().onVerify(null,{validate:proof});
assert.equal(dispatched.filter(x=>x.payload?.secureCaptcha===proof).length,3);
ours.options().onVerify(null,{validate:proof});
assert.equal(new URL(ours.context.location.href,'https://cumulus-captcha.localhost').searchParams.get('validate'),proof);
const first=ours.context.location.href;
ours.options().onVerify(null,{validate:'second'});ours.options().onClose();
assert.equal(ours.context.location.href,first);
for(const kind of ['close','load_error','verify_error']) {
  const env=environment();vm.runInNewContext(fs.readFileSync('desktop/src/captcha/page.js','utf8'),env.context);
  if(kind==='load_error')env.scripts[0].onerror();
  else {env.scripts[0].onload(); if(kind==='close')env.options().onClose();else env.options().onVerify({code:1},null);}
  assert.equal(new URL(env.context.location.href,'https://cumulus-captcha.localhost').searchParams.get('status'),kind==='close'?'cancelled':'unavailable');
}
console.log(JSON.stringify({ok:true,officialSource:officialPath,sha256:crypto.createHash('sha256').update(fs.readFileSync(officialPath)).digest('hex'),checks:['fixed_options','official_three_callbacks','opaque_validate','single_completion','close','loader_failure','verification_failure'],networkRequests:0}));
