// Optional local source oracle; public CI consumes frozen synthetic vectors.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const parser=require('../../frontend/node_modules/@babel/parser');
const root=path.resolve(process.argv[2]||'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names=['hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js','hybrid_app.chunk.1482700000000.js/15.js','hybrid_app.chunk.1482700000000.js/48.js'];
const src=names.map(n=>fs.readFileSync(path.join(root,n),'utf8')),ast=src.map(s=>parser.parse(s));
function walk(n,fn){if(!n||typeof n!=='object')return;fn(n);for(const v of Object.values(n))Array.isArray(v)?v.forEach(c=>walk(c,fn)):walk(v,fn);}
const code=(n,i=0)=>src[i].slice(n.start,n.end);
let serializer;walk(ast[2],n=>{if(n.type==='FunctionDeclaration'&&n.id.name==='f')serializer=n;});assert(serializer);
const form=vm.runInNewContext('('+code(serializer,2)+')',{i:{a:v=>v!==null&&typeof v==='object'&&!Array.isArray(v)},s:encodeURIComponent,u:v=>'['+v.map(encodeURIComponent).join(',')+']',d:v=>encodeURIComponent(JSON.stringify(v))});
const vectors={};
for(const [name,label]of [['sendEmailCaptcha','Send'],['verifyEmailCaptcha','Verify']]){
 const found=[];walk(ast[0],n=>{if(n.type==='ObjectExpression'&&n.properties.some(p=>p.key?.name==='key'&&p.value?.value===name)){const f=n.properties.find(p=>p.key?.name==='value').value;if(code(f).includes('"/api/user/2fa/securityEmail/'))found.push(f);}});assert.equal(found.length,1);
 let fetch;walk(found[0],n=>{if(n.type==='CallExpression'&&n.callee.property?.name==='fetch')fetch=n;});assert(fetch);
 const env={e:'SECURITY_EMAIL',n:'123456',v:['SECURITY_EMAIL'],m:['SECURITY_EMAIL','123456']};
 const scene=fetch.arguments[1].properties.find(p=>p.key.name==='scene').value.name;
 let init,route;walk(found[0],n=>{if(n.type==='AssignmentExpression'&&n.left.name===scene&&n.right.type==='ConditionalExpression'&&n.right.alternate.value==='0')init=n.right;if(n.type==='AssignmentExpression'&&n.left.name===fetch.arguments[0].name&&n.right.type==='ConditionalExpression')route=n.right;});assert(init&&route);
 env[scene]=vm.runInNewContext('('+code(init)+')',env);
 const payload=vm.runInNewContext('('+code(fetch.arguments[1])+')',env);payload.fromPage='JSK';
 vectors['jsk'+label]={path:vm.runInNewContext('('+code(route)+')',env),payload:Object.fromEntries(new URLSearchParams(form(payload)))};
 const legacy=[];walk(ast[1],n=>{if(n.type==='VariableDeclarator'&&n.init?.type==='FunctionExpression'&&code(n.init,1).includes('"api/user/2fa/securityEmail/'+(label==='Send'?'send':'verify')+'"'))legacy.push(n.init);});assert.equal(legacy.length,1);
 const run=vm.runInNewContext('('+code(legacy[0],1)+')',{o:{a:config=>payload=>({path:'/'+config.url,payload:Object.fromEntries(new URLSearchParams(form(payload)))})},g:null});
 vectors['legacy'+label]=label==='Send'?run():run(0,'123456');
}
assert.equal(vectors.jskSend.payload.scene,'0');assert.equal(vectors.legacyVerify.payload.captcha,'123456');
console.log(JSON.stringify({sources:Object.fromEntries(names.map((n,i)=>[n,crypto.createHash('sha256').update(src[i]).digest('hex')])),vectors},null,2));
