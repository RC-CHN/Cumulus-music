// Read-only audit of the actual WebView proof, using pinned official field code.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto');
const root=path.resolve(process.argv[2]);
const source='references/netease-3.1.42.205594-analysis/protocol/watchman-remote-602a5ad7-20261027/';
const code=fs.readFileSync(source+'watchman.readable.js','utf8');
const hash=s=>crypto.createHash('sha256').update(s).digest('hex');
if(hash(code)!=='54ddd682e2fddbeec8a86550af1224d98f3b1e305e6198b8c68e8116d00d925c'||hash(fs.readFileSync(source+'watchman.min.js'))!=='343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5')throw Error('source changed');
const fn=name=>{const a=code.indexOf('        function '+name+'('),b=code.indexOf('\n        }',a);if(a<0||b<0)throw Error(name);return code.slice(a,b+10).trim();};
const table=code.slice(code.indexOf('          da = {')+15,code.indexOf('\n          fc = 0,')).trim().replace(/,$/,'');
const ctx=vm.createContext({aa:encodeURIComponent,oa:parseInt,Tc:Object.prototype.toString,B:'BOOLEAN',z:'NUMBER',Z:'HEX',u:'STRING',I:'ARRAY'});
vm.runInContext(['G','Y','O','ga','Ib','Aa','xa'].map(fn).join('\n')+'\nvar da='+table+';',ctx);
const captured=JSON.parse(fs.readFileSync(path.join(root,'captured.json')));
const complete=JSON.parse(fs.readFileSync(path.join(root,'complete.json')));
const check=x=>{if(!x)throw Error('proof mismatch');};
check(captured.ok && captured.trustedKeySamples>=2 && captured.trustedClickSamples>=2 && captured.typedLength>=8);
let keySamples=0;
for(const field of captured.fields){
 check(field.key.startsWith('_'));ctx.key=field.key;ctx.value=field.value;
 const official=Buffer.from(vm.runInContext('xa(value,da[key])',ctx,{timeout:2000}).map(x=>x&255));
 check(official.equals(Buffer.from(field.bytes)));
 if(field.key==='_keydown'){check(field.value.length===4);check(field.value[2]===2);check(field.value[3]==='watchman-proof-input');keySamples++;}
}
check(keySamples===captured.trustedKeySamples);
check(complete.ok && complete.realWebview && complete.trustedOsInput && complete.programmaticInputMarkedUntrusted && complete.staleGenerationRejected);
check(complete.networkRequests===0 && complete.sdkTokenRequests===0 && complete.inputContentsCollected===false);
check(!JSON.stringify(captured).includes('samplepassword0123'));
const result={ok:true,officialEncodedFields:captured.fields.length,trustedKeySamples:keySamples,receivedFrames:captured.receivedFrames,networkRequests:0,inputContentsAbsent:true,staleGenerationRejected:true};
fs.writeFileSync(path.join(root,'independent-audit.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(result));
