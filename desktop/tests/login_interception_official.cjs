// Executes original M/1368 control flow and M/34 gate; no network or music SDK.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const parser=require('../../frontend/node_modules/@babel/parser');
const base='references/netease-3.1.42.205594-analysis/recovered-js/modules/';
const vendorPath=base+'hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js';
const gatePath=base+'hybrid_app.chunk.1482700000000.js/34.js';
const bridgePath=base+'hybrid_app.chunk.1482700000000.js/290.js';
const bridgeSource=fs.readFileSync(bridgePath,'utf8');
const vendor=fs.readFileSync(vendorPath,'utf8'), gateSource=fs.readFileSync(gatePath,'utf8');
function walk(n, visit, parent) { if(!n||typeof n!=='object')return;visit(n,parent);for(const v of Object.values(n))if(Array.isArray(v))v.forEach(x=>walk(x,visit,n));else if(v&&typeof v==='object')walk(v,visit,n); }
let block,method,gate,normalizer,fetchMethod,bridgeMethod;
walk(parser.parse(vendor),(n,p)=>{
 if(n.type==='FunctionDeclaration'&&n.id.name==='O'&&p.type==='BlockStatement')block=p;
 if(n.type==='ObjectExpression'&&n.properties.some(v=>v.key?.name==='key'&&v.value?.value==='refreshInvalidToken')){const v=n.properties.find(v=>v.key?.name==='value').value;method=vendor.slice(v.start,v.end);}
 if(n.type==='RegExpLiteral'&&n.pattern==='^.*?\\/(e?api)')normalizer=vendor.slice(n.start,n.end);
 if(n.type==='ObjectExpression'&&n.properties.some(v=>v.key?.name==='key'&&v.value?.value==='fetch')){const v=n.properties.find(v=>v.key?.name==='value')?.value;if(v&&vendor.slice(v.start,v.end).includes('fromPage: "JSK"'))fetchMethod=vendor.slice(v.start,v.end);}
});
walk(parser.parse(bridgeSource),n=>{if(n.type==='ObjectProperty'&&n.key.name==='nativeRequest'&&n.value.type==='ArrowFunctionExpression')bridgeMethod=bridgeSource.slice(n.value.start,n.value.end);});
walk(parser.parse(gateSource),n=>{if(n.type==='FunctionDeclaration'&&n.id.name==='p')gate=gateSource.slice(n.start,n.end);});
assert(block&&method&&gate&&normalizer&&fetchMethod&&bridgeMethod);
const helpers=block.body.filter(n=>n.type==='FunctionDeclaration').map(n=>vendor.slice(n.start,n.end)).join('\n');
const u={music_u:true,music_a:false},a={music_u:false,music_a:true};
const ok=action=>({code:200,data:{action}}),bad={code:500};
const scenarios=[
 {name:'no_token',steps:[{tokens:{music_u:false,music_a:false},configError:true}]},
 {name:'recursive',steps:[{url:'https://interface.music.163.com/eapi/middle/account/token/refresh',configError:true}]},
 {name:'anonymous_whitelist',steps:[{tokens:a,config:{config:['https://music.163.com/api/test']}},{tokens:{music_u:true,music_a:true},config:{config:['/eapi/test']}},{tokens:u,config:{config:['/api/test']},response:ok(1)}]},
 {name:'daily_not_relevant_and_clock_backward',steps:[{now:100000,response:ok(1)},{now:104999},{now:99999},{now:105000,response:ok(1)}]},
 {name:'valid_time_is_request_start',steps:[{now:100000,finish:106000,response:ok(1)},{now:106000,response:ok(1)}]},
 {name:'business_failure_threshold',steps:[{response:bad},{response:bad},{response:bad},{response:bad}]},
 {name:'transport_alone_does_not_logout',steps:[{error:true},{error:true},{error:true},{response:bad}]},
 {name:'config_error_counts',steps:[{configError:true},{config:{config:'bad'}},{response:bad}]},
 {name:'success_resets_failure_counter',steps:[{response:bad},{response:bad},{response:ok(3)},{response:bad}]},
 {name:'strict_action_types',steps:[{response:ok(null)},{response:ok('2')},{response:ok(3)},{response:{code:200,data:{}}},{response:{code:200,data:{action:2,title:'重新登录',message:'请确认账号'}}}]},
 {name:'strict_response_code',steps:[{response:{code:'200',data:{action:1}}}]},
 {name:'ordinary_config_objects_are_empty',steps:[{config:{config:{}},response:ok(1)}]},
 {name:'malformed_whitelist_entry',steps:[{config:{config:[1]},response:ok(1)}]},
 {name:'truthy_object_length_throws',steps:[{config:{config:{length:'1'}}},{config:{config:{length:-1}}},{response:bad}]},
];
(async()=>{
 const output=[];
 for(const scenario of scenarios){
  let now=100000,step,events=[],monitor=[],calls=[];
  const c={Ve:{log(){},error(){}},Ue:{LOGIN_INVALID_EVENT:'LOGIN_INVALID_EVENT'},Date:{now:()=>now}};
  vm.createContext(c);vm.runInContext(helpers+'\nJe=function(...a){record(...a)};globalThis.run=('+method+');',Object.assign(c,{record:(...v)=>monitor.push(v)}),{timeout:1000});
  const state={isRefreshInvalidTokenProcessing:false,refreshInvalidTokenErrorCount:0,lastValidTokenTime:0,
   eventEmitter:{emit:e=>events.push(e)},getTokenInfo:async()=>({MUSIC_U:(step.tokens??u).music_u?'U':'',MUSIC_A:(step.tokens??u).music_a?'A':''}),
   getPlatform:()=>({getCustomConfig:async()=>{if(step.configError)throw Error('fixture config failure');return step.config??null},fetch:async(path,data,method)=>{calls.push({path,data,method});if(step.finish)now=step.finish;if(step.error)throw Error('fixture transport failure');return {body:step.response??ok(1)}}})};
  const steps=[];
  for(const input of scenario.steps){step=input;now=input.now??now;events=[];monitor=[];calls=[];await c.run.call(state,{payload:{url:input.url??'/api/test'}});
   let expected;const e=events.at(-1)?.payload;
   if(e){if(e.action===undefined||e.action===2)expected={kind:'notice',value:{kind:e.action===2?'reauthenticate':'expired',title:e.title??null,message:e.message}};else expected={kind:'ignored_action'}}
   else if(monitor.some(v=>v[2]==='token_valid'))expected={kind:'valid'};
   else {const skip=monitor.find(v=>v[1]==='skip');if(skip)expected={kind:'skipped',value:({already_processing:'in_progress',no_token:'no_token',is_refresh_api:'refresh_endpoint',whitelist_anonymous:'anonymous_whitelist',time_gap_too_short:'recent_valid'})[skip[2]]};else expected={kind:'failed',value:{count:state.refreshInvalidTokenErrorCount}}}
   steps.push({input,expected,calls});assert.equal(state.isRefreshInvalidTokenProcessing,false);
  }
  output.push({name:scenario.name,steps});
 }
 const gates=[];
 for(const inited of [false,true])for(const jsk of ['', 't1','c'])for(const benefit of ['', 't1','c']) {
  const abtest={'PH-PC-loginJsk':{expGroupName:jsk},'PH-PC-newLoginBenefit':{expGroupName:benefit}};
  const enabled=vm.runInNewContext('('+gate+')()',{a:{a:{inited,getStore:()=>({abtest})}}});
  gates.push({inited,jsk,benefit,enabled});
 }
 const paths=['/api/test','https://host/eapi/test','https://host/apix/test','prefix\n/api/test','prefix\r/api/test','prefix\u2028/api/test','prefix\u2029/api/test','prefix\t/api/test'].map(input=>({input,output:vm.runInNewContext('input.replace('+normalizer+', "")',{input})}));
 const jskRequests=[];
 const fc={Ve:{log(){},error(){}},We:()=>'',Yn:{getInstance:()=>({getMusicTu:()=>''})},o:{Network:{nativeRequest:async(path,options)=>{jskRequests.push({path,options});return {body:{code:200}}}}}};
 vm.createContext(fc);vm.runInContext(helpers+'\nglobalThis.run=('+fetchMethod+');',fc,{timeout:1000});
 await fc.run('/api/login/token/refresh/v1',{refreshToken:'synthetic'},'POST',{});
 await fc.run('/api/middle/account/token/refresh',{fromAPI:'/api/test'},'POST',{});
 const bridgeErrors=[{name:'network',message:'fixture transport failure'},{name:'fetchStatusError',status:401,body:{code:301}},{name:'responseError',body:{code:500}}];
 const bridgeCases=[];
 for(const error of bridgeErrors){let result;const c={k:()=>({url:'/api/test',method:'POST',data:{},header:{}}),a:{b:{info(){},error(){}}},y:{a:()=>async()=>{throw error}},C:Object.assign,Q:async h=>h};const run=vm.runInNewContext('('+bridgeMethod+')',c);await run({path:'/api/test'},v=>{result=v});bridgeCases.push({error,body:result.body,status:result.status});}
 const report={sources:Object.fromEntries([[vendorPath,vendor],[gatePath,gateSource],[bridgePath,bridgeSource]].map(([p,s])=>[p,crypto.createHash('sha256').update(s).digest('hex')])),scenarios:output,gates,paths,jskRequests,bridgeCases};
 const result=JSON.stringify(report,null,2)+'\n';
 if(process.argv[2])fs.writeFileSync(process.argv[2],result);else process.stdout.write(result);
})().catch(e=>{console.error(e);process.exitCode=1});
