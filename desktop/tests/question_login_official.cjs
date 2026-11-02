// Optional original-source oracle; CI uses the resulting frozen, synthetic data.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const parser=require('../../frontend/node_modules/@babel/parser');
const root=path.resolve(process.argv[2]||'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names=['hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js','hybrid_app.chunk.1482700000000.js/15.js','hybrid_app.chunk.1482700000000.js/48.js','hybrid_20.chunk.1482700000000.js/2992.js'];
const src=names.map(n=>fs.readFileSync(path.join(root,n),'utf8')),ast=src.map(s=>parser.parse(s));
function walk(n,fn){if(!n||typeof n!=='object')return;fn(n);for(const v of Object.values(n))Array.isArray(v)?v.forEach(c=>walk(c,fn)):walk(v,fn);}
const code=(n,i=0)=>src[i].slice(n.start,n.end);
let serializer;walk(ast[2],n=>{if(n.type==='FunctionDeclaration'&&n.id.name==='f')serializer=n;});assert(serializer);
const form=vm.runInNewContext('('+code(serializer,2)+')',{i:{a:v=>v!==null&&typeof v==='object'&&!Array.isArray(v)},s:encodeURIComponent,u:v=>'['+v.map(encodeURIComponent).join(',')+']',d:v=>encodeURIComponent(JSON.stringify(v))});
const vectors={},answer='合成设备甲';
for(const [name,label,route]of [['getQuestionData','Read','get'],['verifyQuestion','Answer','verify']]){
 const found=[];walk(ast[0],n=>{if(n.type==='ObjectExpression'&&n.properties.some(p=>p.key?.name==='key'&&p.value?.value===name)){const f=n.properties.find(p=>p.key?.name==='value').value;if(code(f).includes('/api/login/cellphone/twice/question/'+route))found.push(f);}});assert.equal(found.length,1);
 let fetch;walk(found[0],n=>{if(n.type==='CallExpression'&&n.callee.property?.name==='fetch')fetch=n;});assert(fetch);
 const env={};for(const p of fetch.arguments[1].properties){assert.equal(p.value.type,'Identifier');env[p.value.name]=p.key.name==='ans'?answer:undefined;}
 const payload=vm.runInNewContext('('+code(fetch.arguments[1])+')',env);payload.fromPage='JSK';
 vectors['jsk'+label]={path:fetch.arguments[0].value,payload:Object.fromEntries(new URLSearchParams(form(payload)))};
 const vars=[];walk(ast[1],n=>{if(n.type==='VariableDeclarator'&&n.init&&['CallExpression','ArrowFunctionExpression'].includes(n.init.type)&&code(n.init,1).includes('/api/login/cellphone/twice/question/'+route))vars.push(n.init);});assert.equal(vars.length,1);
 const original=vm.runInNewContext('('+code(vars[0],1)+')',{o:{a:c=>(p={})=>({path:c.url,payload:Object.fromEntries(new URLSearchParams(form(p)))})},g:null});
 vectors['legacy'+label]=label==='Read'?original():original({ans:answer});
}
assert.equal(vectors.jskRead.payload.scene,'undefined');assert.deepEqual(vectors.legacyRead.payload,{});
console.log(JSON.stringify({sources:Object.fromEntries(names.map((n,i)=>[n,crypto.createHash('sha256').update(src[i]).digest('hex')])),vectors},null,2));
