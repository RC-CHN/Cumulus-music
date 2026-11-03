// Optional official PC helper oracle. Public CI uses only frozen synthetic values.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const parser=require('../../frontend/node_modules/@babel/parser');
const root=path.resolve(process.argv[2]||'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names=['hybrid_app.chunk.1482700000000.js/363.js','hybrid_app.chunk.1482700000000.js/48.js','hybrid_app.chunk.1482700000000.js/15.js','hybrid_20.chunk.1482700000000.js/2992.js','hybrid_17.chunk.1482700000000.js/2994.js'];
const sources=names.map(name=>fs.readFileSync(path.join(root,name),'utf8'));
function walk(node,fn){if(!node||typeof node!=='object')return;fn(node);for(const value of Object.values(node))Array.isArray(value)?value.forEach(child=>walk(child,fn)):walk(value,fn);}
let serializer;walk(parser.parse(sources[1]),node=>{if(node.type==='FunctionDeclaration'&&node.id.name==='f')serializer=node;});assert(serializer);
const form=vm.runInNewContext('('+sources[1].slice(serializer.start,serializer.end)+')',{
  i:{a:value=>value!==null&&typeof value==='object'&&!Array.isArray(value)},s:encodeURIComponent,
  u:value=>'['+value.map(encodeURIComponent).join(',')+']',d:value=>encodeURIComponent(JSON.stringify(value)),
});
const wire=value=>Object.fromEntries(new URLSearchParams(form(value)));
const routes={N:'/api/cellphone/existence/check',sg:'/api/sms/captcha/verify',rg:'/api/sms/captcha/sent',Kd:'/api/middle/captcha/sent/v1',Xi:'/api/w/register/cellphone'};
for(const [symbol,route]of Object.entries(routes)) {
  const m=sources[2].match(new RegExp('\\.d\\(t, "'+symbol+'", function \\(\\) \\{\\s*return (\\w+);'));
  assert(m);assert(sources[2].includes(m[1]+' = Object(o.a)("'+route+'")'));
}
let group='';
const api=Object.fromEntries(Object.entries(routes).map(([symbol,route])=>[symbol,async(payload)=>({path:route,payload:wire(payload)})]));
const deps={15:api,314:{a:value=>crypto.createHash('md5').update(value).digest('hex')},2:{b:{error:()=>{throw Error('unexpected official validation log');}}},8:{a:{getStore:()=>({abtest:{massagechange:{expGroupName:group}}})}}};
const exports363={};const require363=id=>{assert(id in deps);return deps[id];};require363.d=(target,name,get)=>Object.defineProperty(target,name,{get});
vm.runInNewContext(sources[0],{})(null,exports363,require363);
(async()=>{
  const phone='13800000000',country='86',captcha='246810';
  const vectors={preflight:await exports363.d({cellphone:phone,ctcode:country})};
  for(const [name,flag]of [['sendPlain',''],['sendMiddle','t1']]) {
    group=flag;vectors[name]=await exports363.b({cellphone:phone,ctcode:country,scene:0,secrete:'music_middleuser_pclogin'});
  }
  vectors.verify=await exports363.g({cellphone:phone,captcha,ctcode:country});
  for(const skip of [false,true])vectors[skip?'registerDefault':'registerNamed']=await exports363.f({phone,countryCode:country,captcha,nickname:'社区昵称',force:false,isSkipSetNickname:skip});
  assert(sources[3].includes('200 === e.code && -1 === e.exist'));
  assert(sources[4].includes('force: !1'));
  assert(sources[4].includes('o(i.g, {'));
  const value={sources:Object.fromEntries(names.map((name,index)=>[name,crypto.createHash('sha256').update(sources[index]).digest('hex')])),vectors};
  const target=path.resolve(__dirname,'../../core/tests/fixtures/phone-entry-official.json');
  if(process.argv.includes('--write'))fs.writeFileSync(target,JSON.stringify(value,null,2)+'\n');
  else assert.equal(JSON.stringify(JSON.parse(fs.readFileSync(target))),JSON.stringify(value));
  console.log(JSON.stringify({ok:true,vectors:Object.keys(vectors),scope:'Executes original M363 helpers and M48 form conversion. M2992/2994 control-flow markers checked; no full SDK execution or real login.'}));
})().catch(error=>{console.error(error);process.exitCode=1;});
