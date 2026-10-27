// Offline audit of exactly one private create and its preselected cleanup.
// Account/device IDs and tokens stay in the private input directory.
const fs=require('node:fs'),path=require('node:path');
module.exports=function(root,auditToken){
 const read=name=>JSON.parse(fs.readFileSync(path.join(root,name)));
 const check=(ok,label)=>{if(!ok)throw Error('write audit: '+label);};
 const equal=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
 const plan=read('write-plan.json'),record=read('write-create.json'),owned=read('write-owned.json'),result=read('write-result.json'),pc=read('pc-config.json');
 const files=fs.readdirSync(root).filter(n=>/^write-request-\d+\.json$/.test(n)).sort((a,b)=>Number(a.match(/\d+/)[0])-Number(b.match(/\d+/)[0]));
 const messages=files.map(name=>({request:read(name),reply:read(name.replace('request','response'))}));
 for(const {request:q,reply:r} of messages){
  check(r.status===200&&r.body.code===200,'request acknowledged');
  if(q.route==='eapi'){
   check(!Object.hasOwn(JSON.parse(q.payload.header),'MConfig-Info'),'HTTP-only MConfig');
   check(JSON.parse(q.mconfig).IuRPVVmc3WWul9fT.version===pc.version,'current PC config');
   if(q.antiCheatToken)auditToken(q);
  }
 }
 const creates=messages.filter(m=>m.request.intent==='isolated_private_create');
 const deletes=messages.filter(m=>m.request.intent==='preselected_cleanup_delete');
 check(creates.length===1&&deletes.length===1,'one create and one cleanup');
 check(messages.filter(m=>m.request.intent==='read').every(m=>['/api/w/nuser/account/get','/api/user/playlist','/api/v6/playlist/detail'].includes(m.request.path)),'fixed reads only');
 check(messages.length===messages.filter(m=>m.request.intent==='read').length+2,'no additional mutations');
 const {request:q,reply:r}=creates[0],created=String(r.body.id??r.body.playlist?.id);
 check(q.path==='/api/playlist/create'&&q.route==='eapi'&&q.antiCheatToken,'official EAPI create');
 check(q.payload.name===plan.name&&String(q.payload.uid)===plan.owner&&String(q.payload.privacy)==='10','private planned owner/name');
 check(record.phase==='confirmed'&&record.codes.main===200&&record.playlist_id===created,'journal confirms reply');
 check(owned.id===created&&owned.owner===plan.owner&&owned.name===plan.name&&owned.private&&!owned.favorite&&owned.tracks.length===0,'empty private owned fixture');
 const baseline=plan.baseline.playlists.map(p=>p.id);check(!baseline.includes(created),'new fixture absent from baseline');
 const detail=messages.find(m=>m.request.path==='/api/v6/playlist/detail');check(detail,'independent remote fixture read');
 const playlist=detail.reply.body.playlist;
 check(String(playlist.id)===created&&String(playlist.creator.userId)===plan.owner&&playlist.name===plan.name&&playlist.privacy===10&&playlist.trackIds.length===0,'remote fixture matches');
 const d=deletes[0].request;
 check(d.path==='/api/playlist/delete'&&d.route==='weapi'&&d.payload.pid===created,'separate preselected cleanup');
 check(read('write-delete-intent.json').id===created,'durable cleanup intent');
 // WEAPI ciphertext was not independently decoded: only the guarded native
 // cleanup intent and resulting remote state are proved here.
 const lists=messages.filter(m=>m.request.path==='/api/user/playlist');
 check(lists.length>=3&&lists.every(m=>!m.reply.body.more),'complete raw lists');
 check(equal(lists[0].reply.body.playlist.map(p=>String(p.id)),baseline),'baseline from remote reply');
 check(equal(lists.at(-1).reply.body.playlist.map(p=>String(p.id)),baseline),'remote original IDs and order restored');
 check(equal(read('write-readback-0.json').playlists,plan.baseline.playlists),'original summaries restored');
 check(result.completedAudit&&result.officialCreateAccepted&&result.baselineRestored&&!result.cleanupRequired&&result.createRequests===1&&result.cleanupRequests===1,'reported outcome');
 return {officialCreateCode:200,officialCreateConfirmed:true,createRoute:'eapi',cleanupRoute:'weapi',baselineRestored:true,originalPlaylistCount:baseline.length,businessWrites:2,additionalWrites:0};
};
