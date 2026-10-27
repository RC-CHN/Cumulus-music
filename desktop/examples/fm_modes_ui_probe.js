(async () => {
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const button=t=>all('button').find(n=>n.textContent.trim()===t);
  const source=()=>JSON.parse(el('.fm-page')?.dataset.fmSource||'{"kind":"personal"}');
  const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'fm-proof',payload:{stage,source:source(),...data}});
  const wait=async(fn,label)=>{const end=Date.now()+40000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
  const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'center'});n.click();await sleep(200);};
  const state=()=>el('.playerbar')?.dataset.state;
  const position=()=>Number(el('[aria-label="播放进度"]')?.value);
  const title=()=>el('.now-copy button')?.textContent.trim();
  async function seek(value){const slider=el('[aria-label="播放进度"]');await wait(()=>slider&&!slider.disabled,'seek_enabled');slider.value=String(value);slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(position()-value)<.6,'seek_done');}
  async function pause(){if(state()==='playing')await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>state()==='paused','paused');}
  async function stage(name,data={}){el('.main-content').scrollTo(0,0);await sleep(500);const main=el('.main-content');if(document.documentElement.scrollWidth>innerWidth||main.scrollWidth>main.clientWidth+1)throw Error('layout_overflow');window.__fmAck='';await report(name,{overflow:false,width:innerWidth,height:innerHeight,...data});await wait(()=>window.__fmAck===name,'native_ack');await sleep(1000);}
  let step='boot';
  try{
    await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account');
    if(window.__fmReopen){
      await click(button('私人漫游'));await wait(()=>source().kind==='zone'&&button('继续播放')&&state()==='paused'&&Math.abs(position()-14)<.7,'zone_reopen');await stage('mode_reopened');
      await click(button('继续播放'));await wait(()=>state()==='playing'&&position()>14.2,'zone_resume');await stage('mode_reopened_playing');
      await pause();await click(button('返回原播放队列'));await wait(()=>state()==='paused'&&Math.abs(position()-12)<.7,'backup_restore');await stage('mode_reopened_returned');
      await report('complete',{ok:true,screenshots:3,audioOutput:'null',remoteMutations:0});return;
    }
    await wait(()=>el('.track-row')&&button('播放全部'),'daily');await click(button('播放全部'));await wait(()=>state()==='playing'&&position()>.3,'normal_play');await pause();await seek(12);const original=title();await stage('mode_original');
    step='cards';await click(button('私人漫游'));await wait(()=>button('切换漫游')&&!el('.fm-page .error-state'),'fm_ready');await click(button('切换漫游'));await wait(()=>el('[data-fm-kind="familiar"]'),'familiar_card');const compactCount=all('[data-zone]').length;if(compactCount!==5)throw Error('compact_zone_count');await click(button('更多专区'));const totalZoneCount=all('[data-zone]').length;if(totalZoneCount<=5)throw Error('more_zones');await click(button('收起专区'));await stage('mode_cards',{zoneCount:all('[data-zone]').length,totalZoneCount});
    step='familiar';await click(el('[data-fm-play="familiar"]'));await wait(()=>state()==='playing'&&position()>.3&&button('暂停漫游'),'familiar_play');await stage('mode_familiar');await pause();
    step='zone';await click(button('切换漫游'));await wait(()=>el('[data-zone]'),'zone_links');await click(el('[data-zone]'));await wait(()=>el('[data-fm-kind="zone"]'),'zone_card');await stage('mode_zone_cards');await click(el('[data-fm-kind="zone"]'));await wait(()=>source().kind==='zone'&&button('开始漫游')&&!el('.fm-page .error-state'),'zone_ready');await click(button('开始漫游'));await wait(()=>state()==='playing'&&position()>.3&&button('暂停漫游'),'zone_play');const zoneFirst=title();await stage('mode_zone');
    step='zone_eof';await pause();await seek(Number(el('[aria-label="播放进度"]').max)-.8);await click(el('.playerbar [aria-label="播放"]'));await wait(()=>state()==='playing'&&title()!==zoneFirst&&position()<8,'zone_auto_next');await stage('mode_zone_advanced');await pause();
    step='return';await click(button('返回原播放队列'));await wait(()=>state()==='paused'&&title()===original&&Math.abs(position()-12)<.7,'original_restored');await stage('mode_returned');
    step='save';await click(button('开始漫游'));await wait(()=>state()==='playing'&&button('暂停漫游'),'zone_reenter');await pause();await seek(14);await stage('mode_saved');
    await report('complete',{ok:true,screenshots:8,audioOutput:'null',remoteMutations:0});
  }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'unknown'});}
})();
