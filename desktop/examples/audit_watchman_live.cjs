// Independent offline proof: inverse packet -> ORIGINAL encoder; original
// M/402 matching and original SDK token encoder; SQLite/native reply binding.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto');
const root=path.resolve(process.argv[2]);
const read=name=>JSON.parse(fs.readFileSync(path.join(root,name)));
const check=(v,label)=>{if(!v)throw Error('live audit: '+label);};
const sha=s=>crypto.createHash('sha256').update(s).digest('hex');
const base='references/netease-3.1.42.205594-analysis/protocol/watchman-remote-602a5ad7-20261027/';
const code=fs.readFileSync(base+'watchman.readable.js','utf8'),constants=JSON.parse(fs.readFileSync(base+'codec-constants.json'));
check(sha(code)==='54ddd682e2fddbeec8a86550af1224d98f3b1e305e6198b8c68e8116d00d925c','readable source');
check(sha(fs.readFileSync(base+'watchman.min.js'))==='343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5','SDK source');
const fn=name=>{const a=code.indexOf('        function '+name+'('),b=code.indexOf('\n        }',a);check(a>=0&&b>a,'function '+name);return code.slice(a,b+10).trim();};
const names=['G','Y','O','ga','Ib','Aa','xa','hb','Wb','gb','Na','Cc','ha','Xb','ib','fb','Vb','Ub','Yb','Zc','Zb','$c','Xa','ad','Qb','Sb','Tb','kb','ia','J','L','M','N','ac','bc','cc','ec','mb','Lc'];
let random=[],did='';const math=Object.create(Math);math.random=()=>{check(random.length,'seed exhausted');return random.shift()/256;};
const ctx=vm.createContext({aa:encodeURIComponent,$b:decodeURIComponent,dc:unescape,oa:parseInt,Tc:Object.prototype.toString,B:'BOOLEAN',z:'NUMBER',Z:'HEX',u:'STRING',I:'ARRAY',Rb:[...constants.alphabet],Wc:[...'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'],Yc:constants.substitution.map(v=>v>127?v-256:v),eb:'3',Xc:'=',jb:64,T:64,Ma:4,lb:4,Math:math,C:{state:{options:{za:1}}},ta:'did',fa:()=>({k:()=>did})});
vm.runInContext(names.map(fn).join('\n'),ctx);
const call=(name,...args)=>{ctx.args=args;return vm.runInContext(name+'(...args)',ctx,{timeout:2000});};
const alphabet=constants.alphabet+'3',standard='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=';
const inverse=Array.from({length:256},(_,n)=>constants.substitution.indexOf(n));
function packet(encoded){
 const raw=Buffer.from([...encoded].map(c=>{check(alphabet.includes(c),'alphabet');return standard[alphabet.indexOf(c)];}).join(''),'base64');
 check(raw.length>=68&&(raw.length-4)%64===0,'blocks');
 const seed=raw.subarray(0,4),key=Array.from({length:64},(_,n)=>constants.secret.charCodeAt(n%constants.secret.length)^seed[n%4]);
 let previous=key,out=[];
 for(let start=4;start<raw.length;start+=64){const block=raw.subarray(start,start+64);for(let n=0;n<64;n++){
  const x=((((inverse[inverse[block[n]]]^previous[n])-previous[n])&255)^key[n]);
  out.push(((x-(-70+n))&255)^((91-n)&255));
 }previous=block;}
 const data=Buffer.from(out),length=data.readUInt32BE(data.length-4);check(length>=8&&length<=data.length-4,'length');check(data.subarray(length,data.length-4).every(v=>v===0),'padding');
 const payload=data.subarray(0,length-8);random=[...seed];check(call('kb',[...payload])===encoded,'original packet encoder incl CRC');
 const fields=new Map();for(let p=0;p<payload.length;){check(p+4<=payload.length,'field header');const id=payload.readUInt16BE(p),n=payload.readUInt16BE(p+2);p+=4;check(p+n<=payload.length,'field body');const previous=fields.get(id)??[];previous.push(payload.subarray(p,p+n));fields.set(id,previous);p+=n;}
 return fields;
}
const complete=read('complete.json'),pc=read('pc-config.json');check(complete.ok&&complete.musicRequestContextConnected,'completed connected proof');
const writesExecuted=complete.businessWrites>0;
const requests=fs.readdirSync(root).filter(n=>/^watchman-request-\d+\.json$/.test(n)).sort((a,b)=>Number(a.match(/\d+/)[0])-Number(b.match(/\d+/)[0]));
check(requests.length===(complete.playlistAudit?complete.watchmanRequests:(writesExecuted?5:3))&&requests.length<=512,'config/device/behavior count');let device,deviceReply,config;const behaviors=[];
for(const name of requests){const q=read(name),reply=read(name.replace('request','response'));check(['ac.dun.163.com','ac.dun.163yun.com'].includes(q.host),'reviewed host');
 if(q.path==='/v2/config/js'){check(reply.code===200,'SDK config');config=JSON.parse(call('Cc',reply.result.conf));check(reply.result.v==='2.7.5_602a5ad7','reviewed build');}
 else {check(reply[0]===200,'accepted '+q.path);const fields=packet(q.details.packet);check(fields.get(1)[0].toString()==='2.7.5_602a5ad7','packet build');check(fields.get(9)[0].toString()==='YD00000558929251','packet product');
  if(q.path==='/v3/d'){check(!device,'one device report');device=fields;deviceReply=reply;}else{check(q.path==='/v3/b','behavior path');behaviors.push(fields);}
 }
}
check(device&&behaviors.length&&config,'all report stages');
const m0=read('music-request-0.json'),m1=read('music-request-1.json'),m2=read('music-request-2.json');
const r0=read('music-response-0.json'),r1=read('music-response-1.json'),r2=read('music-response-2.json');
check(m0.path==='/api/w/nuser/account/get'&&m1.path==='/api/user/playlist'&&m2.path==='/api/v6/playlist/detail','fixed read routes');
for(const r of [r0,r1,r2])check(r.status===200&&r.body.code===200,'music reply');
const key='IuRPVVmc3WWul9fT',info=JSON.parse(r0.metadata['mconfig-info'])[key];check(info.os==='pc'&&pc.version===info.version&&pc.source_url===info.fullUrl,'PC provenance');
check(JSON.parse(m0.mconfig)[key].version===0,'initial negotiation');for(const m of [m1,m2])check(JSON.parse(m.mconfig)[key].version===pc.version,'updated request version');
for(const m of [m0,m1,m2])check(!Object.hasOwn(JSON.parse(m.payload.header),'MConfig-Info'),'MConfig middleware ordering');
const module402=fs.readFileSync('references/netease-3.1.42.205594-analysis/recovered-js/modules/hybrid_app.chunk.1482700000000.js/402.js','utf8');
check(sha(module402)==='c759837f6de9c3f6b108edd1b9c22da8202d8cf7bb861f58454f7f4b9a054a37','M402 source');
const routing=vm.createContext({L:{a:{apiDomain:'https://interfacepc.music.163.com'}},f:{a:{configFromRequest$:{getValue:()=>pc.data}}}});
vm.runInContext(module402.slice(module402.indexOf('  const B = e => {'),module402.indexOf('  function U(e)')),routing);
const usedNonces=new Set();
function auditToken(request){
 routing.argument=request.path;const business=vm.runInContext('z(argument)',routing);
 check(typeof business==='string'&&business.length,'protected original path match');
 const token=request.antiCheatToken;check(token===request.payload.checkToken&&token===JSON.parse(request.payload.header)['X-antiCheatToken'],'both token fields');
 const decoded=JSON.parse(call('Cc',token));did=deviceReply[3];check(decoded.d===did&&!Object.hasOwn(decoded,'t'),'accepted device, not fallback token');
 const matches=behaviors.filter(fields=>fields.get(3)[0].toString()===decoded.b);check(matches.length===1,'one matching behavior nonce');
 const fields=matches[0];check(fields.get(2)[0].toString()===business,'original last-match business');
 ctx.C.state.options.za=config.ret??1;check(call('ec',{C:decoded.b,la:false})===token,'original token encoder');
 check(fields.get(8)[0].toString()===did&&fields.get(7)[0].toString()===deviceReply[2],'behavior uses accepted device');
 check(!usedNonces.has(decoded.b),'fresh token for each request');usedNonces.add(decoded.b);
 return business;
}
function auditRequest(request){
 routing.argument=request.path;const business=vm.runInContext('z(argument)',routing);
 if(business)return auditToken(request);
 check(!request.antiCheatToken&&!request.payload.checkToken&&!JSON.parse(request.payload.header)['X-antiCheatToken'],'unprotected request has no injected token');
 return null;
}
check(auditToken(m2)===complete.protectedBusiness,'reported read business');
const uid=r0.body.profile.userId,pid=r2.body.playlist.id;check(r2.body.playlist.creator.userId===uid&&r1.body.playlist.some(p=>p.id===pid&&p.creator.userId===uid),'own resource');
const stored=read('sqlite-audit.json');check(stored.ok&&stored.persistedConfigAndDeviceVerified,'independent SQLite audit');
for(const input of stored.inputs)check(sha(fs.readFileSync(path.join(root,input.path)))===input.sha256,'SQLite audit input changed');
const writes=complete.playlistAudit&&!complete.playlistAudit.recoveredOriginalJournal?require('./audit_watchman_playlist.cjs')(root,auditRequest):(writesExecuted?require('./audit_watchman_write.cjs')(root,auditToken):null);
check(usedNonces.size===behaviors.length,'all behavior reports accounted for');
const summary={ok:true,originalPacketComparisons:requests.length-1,originalTokenComparison:true,originalTokenComparisons:usedNonces.size,originalPathMatcher:true,actualPcVersion:pc.version,acceptedDeviceCode:deviceReply[0],acceptedBehaviorCode:200,protectedReadCode:r2.body.code,ownResourceVerified:true,persistedConfigAndDeviceVerified:true,musicReads:3,businessWrites:writes?.businessWrites??0,writeAudit:writes,networkRequestsByAudit:0};
const output=process.argv[3]??'independent-audit.json';check(/^independent-audit(?:-[a-z0-9]+)?\.json$/.test(output),'audit output name');
fs.writeFileSync(path.join(root,output),JSON.stringify(summary,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(summary));
