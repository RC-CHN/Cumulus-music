// Original encoders independently check native collection output. No network.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto');
const root=path.resolve(process.argv[2]),proof=JSON.parse(fs.readFileSync(path.join(root,'measurements.json'))),complete=JSON.parse(fs.readFileSync(path.join(root,'complete.json')));
const sha=x=>crypto.createHash('sha256').update(x).digest('hex');
const check=x=>{if(!x)throw Error('native collection audit failed');};
let fields=0,events=0;
for(const [version,directory,rawHash,readableHash,old] of [
 ['2.7.5_af2952a4','watchman-remote','af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787','8edd86c297d2aa0e28d80084482e511c990089cdd2b5ca8b158c2f88f1776359',true],
 ['2.7.5_602a5ad7','watchman-remote-602a5ad7-20261027','343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5','54ddd682e2fddbeec8a86550af1224d98f3b1e305e6198b8c68e8116d00d925c',false],
]){
 const source='references/netease-3.1.42.205594-analysis/protocol/'+directory+'/';
 const code=fs.readFileSync(source+'watchman.readable.js','utf8');check(sha(code)===readableHash&&sha(fs.readFileSync(source+'watchman.min.js'))===rawHash);
 const fn=name=>{const a=code.indexOf('        function '+name+'('),b=code.indexOf('\n        }',a);check(a>=0&&b>a);return code.slice(a,b+10).trim();};
 const names=old?['B','W','M','ea','Hb','ya','va']:['G','Y','O','ga','Ib','Aa','xa'];
 const ctx=vm.createContext(old?{Y:encodeURIComponent,ma:parseInt,Sc:Object.prototype.toString,A:'BOOLEAN',y:'NUMBER',X:'HEX',t:'STRING',F:'ARRAY'}:{aa:encodeURIComponent,oa:parseInt,Tc:Object.prototype.toString,B:'BOOLEAN',z:'NUMBER',Z:'HEX',u:'STRING',I:'ARRAY'});
 const tableName=old?'ba':'da',table=code.slice(code.indexOf('          '+tableName+' = {')+15,code.indexOf(old?'\n          ec = 0,':'\n          fc = 0,')).trim().replace(/,$/,'');
 vm.runInContext(names.map(fn).join('\n')+'\nvar '+tableName+'='+table+';',ctx);
 const report=proof.reports.find(r=>r.build===version);check(report&&report.deviceCacheReused&&report.device.length===38);
 check(report.device.filter(f=>f.failed).map(f=>f.key).sort().join(',')==='ib,jb');
 check(report.device.find(f=>f.key==='La').value==='');
 check(report.individualEnvironment.length===21&&report.individualTimingInputsAreSynthetic);
 check(report.individualEnvironment.filter(f=>f.failed).map(f=>f.key).join(',')==='Xb');
 check(report.events.some(f=>f.key==='_click'&&f.value[2]===3));
 if(report.cachedDevice)check(JSON.stringify(report.cachedDevice)===JSON.stringify(report.device));
 for(const rows of [report.device,report.cachedDevice??[],report.deviceEnvironment,report.behaviorEnvironment,report.events,report.individualEnvironment,...(old?[]:[proof.afterReload])])for(const row of rows){
  ctx.args=[row.value,ctx[tableName][row.key],row.failed];
  const bytes=Buffer.from(vm.runInContext((old?'va':'xa')+'(...args)',ctx).map(v=>v&255));
  check(bytes.toString('hex')===row.hex);fields++;if(row.key.startsWith('_'))events++;
 }
}
check(complete.ok&&complete.actualPageReload&&complete.stalePendingCancelled&&complete.staleFrameRejected&&complete.deviceCacheRebuiltAfterReload&&complete.nativeCollectionConnected);
check(complete.networkRequests===0&&complete.deviceReports===0&&complete.behaviorReports===0&&complete.deviceReads===114);
if(proof.sourceCalls)check(proof.sourceCalls.filter(s=>s.startsWith('d:')).length===114);
const result={ok:true,officialEncodedFields:fields,eventFields:events,builds:2,actualDeviceReads:114,actualPageReload:true,cacheReusedAndRebuilt:true,missingLoaderPreserved:true,fontFailuresPreserved:true,audioEmptyResultPreserved:true,networkRequests:0};
fs.writeFileSync(path.join(root,'independent-audit.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(result));
