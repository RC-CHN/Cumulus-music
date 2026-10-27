// Read-only official-function audit of real WebView measurements and Rust output.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto');
const root=path.resolve(process.argv[2]);
const proof=JSON.parse(fs.readFileSync(path.join(root,'measurements.json')));
const complete=JSON.parse(fs.readFileSync(path.join(root,'complete.json')));
const hash=x=>crypto.createHash('sha256').update(x).digest('hex');
const check=x=>{if(!x)throw Error('measurement proof mismatch');};
let measured=0,failed=0,unsupported=0,deviceFields=0;
for(const [build,directory,rawHash,readableHash,old] of [
 ['2.7.5_af2952a4','watchman-remote','af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787','8edd86c297d2aa0e28d80084482e511c990089cdd2b5ca8b158c2f88f1776359',true],
 ['2.7.5_602a5ad7','watchman-remote-602a5ad7-20261027','343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5','54ddd682e2fddbeec8a86550af1224d98f3b1e305e6198b8c68e8116d00d925c',false],
]){
 const source='references/netease-3.1.42.205594-analysis/protocol/'+directory+'/';
 const code=fs.readFileSync(source+'watchman.readable.js','utf8');
 check(hash(code)===readableHash&&hash(fs.readFileSync(source+'watchman.min.js'))===rawHash);
 const fn=name=>{const a=code.indexOf('        function '+name+'('),b=code.indexOf('\n        }',a);check(a>=0&&b>a);return code.slice(a,b+10).trim();};
 const names=old?['xa','ca','P','qa','N','D','Gb','B','W','M','ea','Hb','ya','va']:['za','ea','R','sa','P','D','Hb','G','Y','O','ga','Ib','Aa','xa'];
 const ctx=vm.createContext(old?{Y:encodeURIComponent,ma:parseInt,Sc:Object.prototype.toString,A:'BOOLEAN',y:'NUMBER',X:'HEX',t:'STRING',F:'ARRAY'}:{aa:encodeURIComponent,oa:parseInt,Tc:Object.prototype.toString,B:'BOOLEAN',z:'NUMBER',Z:'HEX',u:'STRING',I:'ARRAY'});
 const tableName=old?'ba':'da';
 const table=code.slice(code.indexOf('          '+tableName+' = {')+15,code.indexOf(old?'\n          ec = 0,':'\n          fc = 0,')).trim().replace(/,$/,'');
 vm.runInContext(names.map(fn).join('\n')+'\nvar '+tableName+'='+table+';',ctx);
 const invoke=(name,...args)=>{ctx.args=args;return vm.runInContext(name+'(...args)',ctx,{timeout:2000});};
 for(const row of proof.rows.filter(r=>r.build===build)){
  let expected='',error=false;
  if(row.reading.kind==='fingerprint'){expected=invoke(old?'xa':'za',row.reading.preimage,0);measured++;}
  else if(row.reading.kind==='failed'){expected=null;error=true;failed++;}
  else {check(row.reading.kind==='unsupported');unsupported++;}
  check(row.value===expected&&row.failed===error);
  const bytes=Buffer.from(invoke(old?'va':'xa',expected,ctx[tableName][row.key],error).map(x=>x&255));
  check(row.hex===bytes.toString('hex'));
 }
 const languages=Buffer.from(invoke(old?'va':'xa',proof.languages,ctx[tableName].Ab).map(x=>x&255));
 check(proof.languageHex===languages.toString('hex'));
 for(const row of (proof.device??[]).filter(r=>r.build===build)){
  const bytes=Buffer.from(invoke(old?'va':'xa',row.value,ctx[tableName][row.key],row.failed).map(x=>x&255));
  check(row.hex===bytes.toString('hex'));deviceFields++;
 }
 if(proof.device){
  const get=(mode,key)=>{const row=proof.device.find(r=>r.build===build&&r.mode===mode&&r.key===key);check(row&&!row.failed);return row.value;};
  for(const key of ['Ta','bb','Vb','mb','Nb'])check(JSON.stringify(get('actual',key))===JSON.stringify(get('windows_declared',key)));
  for(const [key,name]of [['$b','userAgent'],['Ia','appVersion'],['Ib','platform'],['zb','language'],['Ab','languages']])check(JSON.stringify(get('windows_declared',key))===JSON.stringify(proof.declaration[name]));
  // The official check must retain the actual engine's eval.toString length.
  const fakeEval=()=>{};fakeEval.toString=()=> 'x'.repeat(proof.host.evalTextLength);
  const a=code.indexOf('          '+(old?'Wa':'Ya')+' = {')+15,b=code.indexOf('\n          '+(old?'Ua':'Wa')+' = [],',a);
  const fields=code.slice(a,b).trim().replace(/,$/,'');
  const nav={userAgent:proof.declaration.userAgent,platform:proof.declaration.platform,plugins:[],productSub:proof.host.productSub};
  const context=vm.createContext(old?{u:nav,x:{},w:{},G:'Windows',O:'Chrome',A:'BOOLEAN',X:'HEX',y:'NUMBER',t:'STRING',F:'ARRAY',Eb:'canvas',Xa:'fonts',Fb:'webgl',eval:fakeEval}:{w:nav,y:{},x:{},H:'Windows',Q:'Chrome',B:'BOOLEAN',Z:'HEX',z:'NUMBER',u:'STRING',I:'ARRAY',Fb:'canvas',Za:'fonts',Gb:'webgl',eval:fakeEval});
  vm.runInContext('var device='+fields+';',context);
  check(get('windows_declared','ob')===vm.runInContext('device.ob.f()',context));
 }
}
check(proof.rows.length===(proof.device?16:8)&&measured>=4&&complete.ok&&complete.realWebview&&complete.networkRequests===0&&complete.deviceReports===0&&complete.behaviorReports===0);
if(proof.device)check(deviceFields===128&&complete.globalIdentityUnchanged&&complete.nativeHardwareFactsPreserved);
const report={ok:true,measuredFingerprintFields:measured,explicitFailedFields:failed,unsupportedFields:unsupported,deviceFields,officialBrowserCheckVerified:!!proof.device,languageFieldsVerifiedForBothBuilds:true,networkRequests:0};
fs.writeFileSync(path.join(root,'independent-audit.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(report));
