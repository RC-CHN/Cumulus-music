(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const el = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const button = t => all('button').find(n => n.textContent.trim() === t);
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'fm-proof',payload:{stage,...data}});
  const wait = async (fn, label) => {const end=Date.now()+40000; while(Date.now()<end){if(fn())return; await sleep(100);} throw Error(label);};
  const click = async n => {if(!n)throw Error('missing_control'); await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'center'});n.click();await sleep(200);};
  const state = () => el('.playerbar')?.dataset.state;
  const position = () => Number(el('[aria-label="播放进度"]')?.value);
  const title = () => el('.now-copy button')?.textContent.trim();
  async function seek(value) {const slider=el('[aria-label="播放进度"]');await wait(()=>slider&&!slider.disabled,'seek_enabled');slider.value=String(value);slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(position()-value)<.6,'seek_done');}
  async function pause() {if(state()==='playing')await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>state()==='paused','paused');}
  async function play() {await click(el('.playerbar [aria-label="播放"]'));await wait(()=>state()==='playing','playing');}
  async function home() {await click(button('私人漫游'));await wait(()=>el('.fm-copy h2')&&!el('.fm-page .error-state'),'fm_home');}
  async function stage(name, data={}) {el('.main-content').scrollTo(0,0);await sleep(500);const main=el('.main-content');if(document.documentElement.scrollWidth>innerWidth||main.scrollWidth>main.clientWidth+1)throw Error('layout_overflow');window.__fmAck='';await report(name,{overflow:false,width:innerWidth,height:innerHeight,...data});await wait(()=>window.__fmAck===name,'native_ack');await sleep(1000);}
  let step='boot';
  try {
    await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account');
    if(window.__fmReopen){
      await home();await wait(()=>state()==='paused'&&Math.abs(position()-14)<.7,'saved_fm_restore');
      await stage('fm_reopened');
      await click(button('继续播放'));await wait(()=>state()==='playing'&&position()>14.2,'fm_resume');await stage('fm_reopened_playing');
      await pause();await click(button('返回原播放队列'));await wait(()=>state()==='paused'&&Math.abs(position()-12)<.7,'normal_backup_restore');await stage('fm_reopened_returned');
      await report('complete',{ok:true,screenshots:3,audioOutput:'null',remoteMutations:0});return;
    }
    step='normal';await wait(()=>el('.track-row')&&button('播放全部'),'daily');await click(button('播放全部'));await wait(()=>state()==='playing'&&position()>.3,'normal_play');await pause();await seek(12);const original=title();await stage('fm_original');
    step='home';await home();await stage('fm_home');
    step='start';await click(button('开始漫游'));await wait(()=>state()==='playing'&&position()>.3&&button('暂停漫游'),'fm_play');const first=title();await stage('fm_playing');
    step='repeat';await pause();await seek(12);await click(el('.playerbar [aria-label="无限漫游"]'));await wait(()=>el('.playerbar [aria-label="单曲循环"]'),'single_mode');await stage('fm_repeat');
    await seek(Number(el('[aria-label="播放进度"]').max)-.8);await play();await wait(()=>state()==='playing'&&position()<8,'single_replay');if(title()!==first)throw Error('repeat_changed_track');await stage('fm_repeat_replayed');
    step='auto';await pause();await click(el('.playerbar [aria-label="单曲循环"]'));await seek(Number(el('[aria-label="播放进度"]').max)-.8);await play();await wait(()=>state()==='playing'&&title()!==first&&position()<8,'natural_advance');await stage('fm_advanced');await pause();
    step='trash';await click(button('减少推荐列表'));await wait(()=>el('.fm-trash-row'),'trash_rows');const check=el('.fm-trash-row input');await click(check);await sleep(2600);if(!check.checked)throw Error('poll_cleared_selection');await stage('fm_trash',{loaded:all('.fm-trash-row').length,selected:all('.fm-trash-row input:checked').length});
    step='leave';await click(button('返回私人漫游'));await click(button('返回原播放队列'));await wait(()=>state()==='paused'&&title()===original&&Math.abs(position()-12)<.7,'original_restored');await stage('fm_returned');
    step='save';await wait(()=>button('开始漫游'),'fm_inactive');await click(button('开始漫游'));await wait(()=>state()==='playing'&&button('暂停漫游'),'fm_reenter');await pause();await seek(14);await stage('fm_saved');
    await report('complete',{ok:true,screenshots:9,audioOutput:'null',remoteMutations:0});
  } catch(e) {await report('failed',{ok:false,step,error:e instanceof Error?e.message:'unknown'});}
})();
