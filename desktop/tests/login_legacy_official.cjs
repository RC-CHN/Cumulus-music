// Execute the recovered M/1384 subscriber, M/12 factory and M/402 code gate.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict'), crypto = require('node:crypto');
const parser = require('../../frontend/node_modules/@babel/parser');
const base = 'references/netease-3.1.42.205594-analysis/recovered-js/modules/hybrid_app.chunk.1482700000000.js/';
const paths = ['1384','12','15','402'].map(id=>base+id+'.js');
const sources = paths.map(p=>fs.readFileSync(p,'utf8'));
function walk(n, f) { if(!n||typeof n!=='object')return; f(n); for(const v of Object.values(n)) if(Array.isArray(v)) v.forEach(x=>walk(x,f)); else if(v&&typeof v==='object')walk(v,f); }
let subscriber, factory, helpers=[], gate;
walk(parser.parse(sources[0]), n=>{
 if(n.type==='CallExpression'&&n.callee.type==='MemberExpression'&&n.callee.property.name==='subscribe'&&n.arguments[0]?.async&&sources[0].slice(n.callee.start,n.callee.end)==='_.a')throw Error('bad selector');
 if(n.type==='CallExpression'&&sources[0].slice(n.callee.start,n.callee.end)==='_.a.subscribe')subscriber=sources[0].slice(n.arguments[0].start,n.arguments[0].end);
});
walk(parser.parse(sources[1]), n=>{
 if(n.type==='AssignmentExpression'&&sources[1].slice(n.left.start,n.left.end)==='t.a'&&n.right.type==='ArrowFunctionExpression')factory=sources[1].slice(n.right.start,n.right.end);
 if(n.type==='FunctionDeclaration')helpers.push(sources[1].slice(n.start,n.end));
});
walk(parser.parse(sources[3]), n=>{
 if(n.type==='FunctionDeclaration'&&n.id.name==='G'){
  const start=n.body.body.findIndex(v=>v.type==='VariableDeclaration'&&v.declarations.some(d=>d.id.type==='ObjectPattern'&&d.id.properties.some(p=>p.key.name==='code'&&p.value.name==='G')));
  assert(start>=0);gate=n.body.body.slice(start).map(v=>sources[3].slice(v.start,v.end)).join('\n');
 }
});
assert(subscriber&&factory&&gate);
assert(sources[2].includes('Object(o.a)("/api/middle/account/token/refresh")'));
const ok=action=>({code:200,data:{action}});
const scenarios=[
 {name:'valid_completion_cooldown',steps:[{now:100,response:ok(1),finish:6100},{now:11099},{now:11100,response:ok(1)}]},
 {name:'null_action_third_failure',steps:[{response:ok(null)},{response:ok(null)},{response:ok(null)},{response:ok(null)}]},
 {name:'missing_action_resets',steps:[{response:ok(null)},{response:ok(null)},{response:{code:200,data:{}}},{response:ok(null)}]},
 {name:'strict_action',steps:[{response:ok('2')},{response:ok(3)},{response:ok(false)},{response:{code:200}}]},
 {name:'explicit_guide_clear',steps:[{response:{code:200,data:{action:2,followAction:1,title:'重新登录',message:'请确认当前账号'}}}]},
 {name:'string_follow_action_keeps_queue',steps:[{response:{code:200,data:{action:2,followAction:'1'}}}]},
 {name:'counter_kept_after_unsupported_action',steps:[{response:{code:500}},{response:{code:500}},{response:ok('1')},{response:ok(null)}]},
 {name:'failure_message',steps:[{response:ok(null)},{response:ok(null)},{response:{code:200,data:{action:null,message:'请再次登录'}}}]},
 {name:'string_code_subscriber_is_strict',steps:[{response:{code:'200',data:{action:1}}}]},
 {name:'transport_original_latch',steps:[{error:true},{response:ok(1)}],difference:true},
];
async function execute(scenario, bridged=false) {
 let now=0, step, calls=[], actions=[], toast=[], timers=[], networkEvents=[];
 const host={uid:'1',isAnonymous:false};
 const dispatch=async action=>{actions.push(action)};
 const ctx={fe:0,pe:false,be:{lastTigger:0,cnt:0,triggered:false},H:{a:{getStore:()=>({host}),getDispatch:()=>dispatch}},S:{b:{warn(){},error(){}}},l:{L:{create:v=>toast.push(v)}},t:dispatch,
  f:{b:async ms=>{now+=ms}},ee:{p:'playingList'},te:{a:{playingList:'playingList'}},setTimeout:(f,ms)=>timers.push({f,at:now+ms})};
 vm.createContext(ctx);
 let fetch=async()=>{calls.push({path:'/api/middle/account/token/refresh',data:{}});if(step.finish!==undefined)now=step.finish;if(step.error)throw Error('synthetic failure');return structuredClone(step.response??ok(1))};
 if(bridged){
  const g=vm.runInNewContext('(function(V){const v=false,t={},h={},j=0,w=0,_=200,I=0,P=0,Q=0,d="/api/middle/account/token/refresh";let k;'+gate+'})',{
   a:{a:v=>typeof v==='string'},l:{isNumber:v=>typeof v==='number'},U(){},m:{b:{warn(){}}},M:Object.assign});
  const request=fetch;
  const c={P:['formatOption','cacheStrategy'],E:{request:async()=>g(await request())},o:{a:v=>typeof v==='string'},r:{a:v=>typeof v==='function'},window:{dispatchEvent(){}},CustomEvent:class{},T:{p:()=>false},x:{a:{next:()=>networkEvents.push('301')}},y:{b:{info(){}}},S:{b(){},a:{network:'network'}}};
  vm.createContext(c);vm.runInContext(helpers.join('\n')+'\nglobalThis.factory=('+factory+');',c);fetch=c.factory('/api/middle/account/token/refresh');
 }
 ctx.v={Hd:fetch}; const run=vm.runInContext('('+subscriber+')',ctx); const steps=[];
 for(const input of scenario.steps){step=input;now=input.now??now;const due=timers.filter(t=>t.at<=now);timers=timers.filter(t=>t.at>now);due.forEach(t=>t.f());calls=[];actions=[];toast=[];let threw=false;try{await run()}catch(e){if(e?.name==='ReferenceError'||e?.name==='TypeError')throw e;threw=true}
  let expected;
  if(threw) expected={kind:'error'};
  else if(actions.some(a=>a.type==='host/logout')){
   const guide=actions.find(a=>a.type==='host/onUpdate')?.payload;
   expected={kind:'notice',value:{notice:{kind:guide?'reauthenticate':'expired',title:guide?.removeGuideTitle??null,message:guide?.removeGuideDesc||toast[0]?.content||'用户验证失效，请重新登录'},clear_queue:actions.some(a=>a.type==='playingList/clearCurPlayingList')}};
  } else if(!calls.length)expected={kind:'skipped',value:'in_progress'};
  else if(ctx.pe)expected={kind:'valid'};
  else if(ctx.fe)expected={kind:'failed',value:{count:ctx.fe}};
  else expected={kind:'ignored_action'};
  steps.push({input,expected,calls,errors:ctx.fe,latched:ctx.pe,actions,toast});
 }
 return {name:scenario.name,difference:scenario.difference??false,steps,networkEvents};
}
(async()=>{
 const results=[];for(const s of scenarios)results.push(await execute(s));
 const bridge=[];for(const code of [200,'200','  +200suffix',500,301,'301','  +301suffix','301.0',null,false,0,'bad','',{},true])bridge.push(await execute({name:'bridge_'+JSON.stringify(code),steps:[{response:{code,data:{action:1}}},{response:ok(1)}]},true));
 assert.equal(bridge[0].steps[0].expected.kind,'valid');assert.equal(bridge[3].steps[0].expected.kind,'error');
 assert(bridge.every(c=>c.steps[0].calls.length===1));
 const report={sources:Object.fromEntries(paths.map((p,i)=>[p,crypto.createHash('sha256').update(sources[i]).digest('hex')])),scenarios:results,bridge};
 fs.writeFileSync(process.argv[2]??'core/tests/fixtures/login_legacy.json',JSON.stringify(report,null,2)+'\n');
 console.log(JSON.stringify({scenarios:results.length,bridgeCases:bridge.length,originalTransportLatch:results.at(-1).steps.every(s=>s.latched)}));
})().catch(e=>{console.error(e);process.exitCode=1});
