// Full private fixture audit, independently bound to raw remote readbacks.
const fs=require('node:fs'),path=require('node:path');
module.exports=function(root,auditRequest){
 const read=n=>JSON.parse(fs.readFileSync(path.join(root,n))),check=(v,s)=>{if(!v)throw Error('playlist audit: '+s);};
 const eq=(a,b)=>JSON.stringify(a)===JSON.stringify(b),ids=list=>list.map(p=>String(p.id));
 const plan=read('playlist-plan.json'),pid=read('playlist-fixture.json').id,pc=read('pc-config.json'),result=read('playlist-result.json');
 const files=fs.readdirSync(root).filter(n=>/^playlist-request-\d+\.json$/.test(n)).sort((a,b)=>Number(a.match(/\d+/)[0])-Number(b.match(/\d+/)[0]));
 const messages=files.map(n=>({q:read(n),r:read(n.replace('request','response'))}));
 const writes=messages.filter(m=>m.q.intent!=='read');
 check(result.ok&&result.workflowPassed&&result.baselineRestored&&!result.cleanupRequired,'successful terminal report');
 check(writes.length===9&&result.businessWrites===9,'nine actual writes');
 check(eq(writes.map(m=>m.q.intent),['create','metadata','add','reorder','remove','order','clear','clear_metadata','delete']),'ordered writes only');
 for(const {q,r} of messages){
  check(q.route==='eapi'&&r.status===200&&r.body.code===200,'all official EAPI acknowledgements');
  check(JSON.parse(q.mconfig).IuRPVVmc3WWul9fT.version===pc.version,'PC config version');
  check(!Object.hasOwn(JSON.parse(q.payload.header),'MConfig-Info'),'HTTP-only configuration');
  auditRequest(q);
 }
 const baseline=plan.baseline.playlists,originalIds=baseline.map(p=>p.id);
 check(!originalIds.includes(pid),'new fixture only');
 const lists=messages.filter(m=>m.q.path==='/api/user/playlist');
 check(lists.every(m=>!m.r.body.more&&String(m.q.payload.uid)===plan.owner),'complete current account lists');
 check(eq(ids(lists[0].r.body.playlist),originalIds)&&eq(ids(lists.at(-1).r.body.playlist),originalIds),'original IDs and order restored');
 check(eq(read('playlist-final-list.json').playlists,baseline),'all original summaries restored');
 for(const {q,r} of writes){
  const label=q.intent,record=read('stage-'+label+'.json'),permit=read('permit-'+label+'.json'),sent=read('sent-'+label+'.json');
  check(record.phase==='confirmed'&&eq(sent.fields,permit.fields)&&q.path===permit.path,'confirmed exact planned intent');
  const payload={...q.payload};delete payload.header;delete payload.checkToken;
  check(eq(payload,permit.fields),'wire equals planned fields');
  if(label==='create'){
   check(q.path==='/api/playlist/create'&&q.payload.name===plan.name&&q.payload.privacy==='10'&&q.payload.uid===plan.owner,'private owned create');
   check(String(r.body.id??r.body.playlist?.id)===pid&&record.playlist_id===pid,'create reply identity');
  }else if(label==='metadata'||label==='clear_metadata'){
   check(q.path==='/api/batch','metadata batch route');
   for(const child of ['/api/playlist/update/name','/api/playlist/tags/update','/api/playlist/desc/update'])check(r.body[child].code===200&&JSON.parse(q.payload[child]).id===pid,'each batch child targets fixture');
   check(record.observed.name===plan.renamed,'metadata readback name');
   check(eq(record.observed.tags,label==='metadata'?['电子']:[])&&record.observed.description===(label==='metadata'?'Rust official EAPI isolated verification':''),'metadata readback fields');
  }else if(label==='order'){
   check(q.path==='/api/playlist/order/update','playlist ordering path');
   const order=JSON.parse(q.payload.ids);check(order.at(-1)===pid,'fixture moved to end');
   check(eq(order.filter(x=>x!==pid),baseline.filter(p=>p.owner===plan.owner).map(p=>p.id)),'original relative order retained');
  }else {
   check(q.payload.pid===pid,'only fixture changed');
   if(label==='delete')check(q.path==='/api/playlist/delete'&&record.observed.playlists.every(p=>p.id!==pid),'official deletion readback');
   else check(q.path==='/api/v1/playlist/manipulate/tracks'&&JSON.parse(q.payload.trackIds).every(id=>plan.tracks.includes(id)),'planned track subset');
  }
  // The service's normalized observation must agree with a subsequent raw
  // list/detail response before any next mutation.
  const index=messages.findIndex(m=>m.q===q),following=[];
  for(const next of messages.slice(index+1)){if(next.q.intent!=='read')break;following.push(next);}
  const observed=record.observed;
  check(following.some(({q,r})=>observed.playlists?
   q.path==='/api/user/playlist'&&eq(observed.playlists.map(p=>p.id),ids(r.body.playlist)):
   q.path==='/api/v6/playlist/detail'&&String(r.body.playlist.id)===pid&&String(r.body.playlist.creator.userId)===plan.owner&&r.body.playlist.privacy===10&&r.body.playlist.name===observed.name&&eq(r.body.playlist.tags,observed.tags)&&(r.body.playlist.description??'')===observed.description&&eq(r.body.playlist.trackIds.map(p=>String(p.id)),observed.tracks)), 'raw readback binds '+label);
 }
 const add=read('stage-add.json'),reorder=read('stage-reorder.json'),remove=read('stage-remove.json'),clear=read('stage-clear.json');
 check(eq([...add.observed.tracks].sort(),[...plan.tracks].sort()),'three tracks added');
 check(eq(reorder.observed.tracks,[...reorder.before.tracks].reverse()),'track order reversed');
 check(eq(remove.observed.tracks,remove.before.tracks.filter(id=>!remove.command.ids.includes(id))),'requested track removed');
 check(clear.observed.tracks.length===0&&clear.command.change==='clear','clear is an actual delete of remaining tracks');
 for(const label of ['duplicate_add','absent_remove'])check(read('stage-'+label+'.json').phase==='unchanged'&&!fs.existsSync(path.join(root,'sent-'+label+'.json')),'no-op sends nothing');
 return {workflowPassed:true,confirmedStages:9,localNoOps:2,businessWrites:9,allWritesEapi:true,officialDeleteConfirmed:true,baselineRestored:true,originalPlaylistCount:baseline.length,compatibilityCleanupUsed:false};
};
