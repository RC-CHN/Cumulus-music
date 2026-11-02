// Optional official-source oracle; public CI reads only its frozen synthetic vectors.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const parser=require('../../frontend/node_modules/@babel/parser');
const root=path.resolve(process.argv[2]||'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names=['hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js','hybrid_app.chunk.1482700000000.js/363.js','hybrid_app.chunk.1482700000000.js/15.js','hybrid_app.chunk.1482700000000.js/48.js','hybrid_19.chunk.1482700000000.js/3138.js','hybrid_app.chunk.1482700000000.js/32.js','hybrid_app.chunk.1482700000000.js/103.js'];
const src=names.map(n=>fs.readFileSync(path.join(root,n),'utf8')),ast=src.map(s=>parser.parse(s));
function walk(n,fn){if(!n||typeof n!=='object')return;fn(n);for(const v of Object.values(n))Array.isArray(v)?v.forEach(c=>walk(c,fn)):walk(v,fn);}
const code=(n,i)=>src[i].slice(n.start,n.end);
let serializer;walk(ast[3],n=>{if(n.type==='FunctionDeclaration'&&n.id.name==='f')serializer=n;});assert(serializer);
const form=vm.runInNewContext('('+code(serializer,3)+')',{i:{a:v=>v!==null&&typeof v==='object'&&!Array.isArray(v)},s:encodeURIComponent,u:v=>'['+v.map(encodeURIComponent).join(',')+']',d:v=>encodeURIComponent(JSON.stringify(v))});
const wire=value=>Object.fromEntries(new URLSearchParams(form(value)));
const vectors={};
for(const [name,route]of [['Read','/api/login/cellphone/twice/used/user/get'],['Confirm','/api/login/cellphone/twice/used/confirm']]){
 const found=[];walk(ast[0],n=>{if(n.type==='CallExpression'&&n.callee.property?.name==='fetch'&&n.arguments[0]?.value===route)found.push(n);});assert.equal(found.length,1);
 const payload=vm.runInNewContext('('+code(found[0].arguments[1],0)+')');payload.fromPage='JSK';vectors['jsk'+name]={path:route,payload:wire(payload)};
 assert(src[2].includes(route));vectors['legacy'+name]={path:route,payload:{}};
}
let registration;walk(ast[1],n=>{if(n.type==='VariableDeclarator'&&n.id.name==='v'&&n.init?.type==='ArrowFunctionExpression'&&code(n.init,1).includes('i.Xi'))registration=n.init;});assert(registration);
assert(src[2].includes('/api/w/register/cellphone'));assert(src[4].includes('o(c.f, a)'));
const register=vm.runInNewContext('('+code(registration,1)+')',{c:Object.assign,o:{a:v=>crypto.createHash('md5').update(v).digest('hex')},i:{Xi:(payload)=>({path:'/api/w/register/cellphone',payload:wire(payload)})}});
(async()=>{
 for(const skip of [false,true])vectors[skip?'registerDefault':'registerNamed']=await register({phone:'13800000000',countryCode:'86',captcha:'246810',nickname:'社区昵称',force:'true',isSkipSetNickname:skip});
 for(const [label,name,symbol,route,args]of [['nicknameHint','p','oe','/api/nickname/hint/get',{}],['nicknameDuplicate','d','ne','/api/nickname/duplicated',{nickname:'社区昵称'}]]){
  let original;walk(ast[4],n=>{if(n.type==='VariableDeclarator'&&n.id.name===name&&n.init?.type==='ArrowFunctionExpression'&&code(n.init,4).includes('r.'+symbol))original=n.init;});assert(original);assert(src[2].includes(route));
  const f=vm.runInNewContext('('+code(original,4)+')',{r:{[symbol]:payload=>({path:route,payload:wire(payload)})}});vectors[label]=await f(args);
 }
 let length;walk(ast[5],n=>{if(n.type==='VariableDeclarator'&&n.id.name==='j'&&n.init&&code(n.init,5).includes('**'))length=n.init;});assert(length);
 const getLength=vm.runInNewContext('('+code(length,5)+')');
 const lengths=['abcd','中文','中a文b','ab-c_','😀😀'].map(input=>({input,length:getLength(input)}));
 assert(src[6].includes('YYYY-MM-DD'));
 console.log(JSON.stringify({sources:Object.fromEntries(names.map((n,i)=>[n,crypto.createHash('sha256').update(src[i]).digest('hex')])),vectors,lengths},null,2));
})().catch(error=>{console.error(error);process.exitCode=1;});
