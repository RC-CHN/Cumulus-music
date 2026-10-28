(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<180;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('.relay-settings button').find(n=>n.textContent.trim()===text);
  const click=n=>{assert(n&&!n.disabled,'button unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const ready=()=>one('.relay-settings')?.getAttribute('aria-busy')==='false';
  const state=()=>one('.playerbar')?.dataset.state;
  const position=()=>Number(one('.playerbar')?.dataset.position??0);
  let stageCount=0;
  async function capture(name,playing=false){
    await sleep(300);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(!document.body.textContent.includes('SYNTHETIC_PRIVATE'),'native alert leaked');
    const card=one('.relay-prompt')?.getBoundingClientRect(),bar=one('.playerbar').getBoundingClientRect(),scroll=one('.settings-scroll')?.getBoundingClientRect();
    if(card)assert(card.left>=0&&card.right<=innerWidth&&card.bottom<=bar.top-3,'relay prompt overlaps player or window');
    if(card){const action=one('.relay-continue');assert(action.scrollWidth<=action.clientWidth,'default relay action text clipped');}
    if(scroll)assert(scroll.bottom<=bar.top+1,'settings content overlaps player');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,prompt:!!card,promptBottom:card?.bottom,playerTop:bar.top,position:position(),playing,dialog:!!one('dialog[open]'),status:one('.relay-settings [role="status"]')?.textContent,operations:all('.relay-operation').map(n=>n.textContent.trim())});await sleep(850);
  }
  async function theme(value){click(one(`input[name="settings-theme"][value="${value}"]`));await wait(()=>document.documentElement.dataset.theme===value,'theme');}
  async function refresh(mode){await emit('fixture',{mode});click(button('检查其他设备'));await sleep(120);await wait(ready,'relay refresh');}
  try{
    await wait(()=>one('.relay-prompt')&&one('.track-row'),'initial relay offer');await capture('initial_light');
    click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&one('input[name="shareSetting"]'),'settings');await theme('light');await capture('settings_light');
    await theme('dark');await refresh('song');await capture('prompt_dark');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark');await theme('light');await capture('small_light');
    await wait(()=>!one('.relay-prompt'),'8 second auto-dismiss');await capture('auto_dismiss');
    one('.relay-settings').scrollIntoView({block:'center'});await refresh('empty');assert(one('.relay-settings').textContent.includes('暂时没有'),'empty device state');await capture('empty');
    await refresh('excluded');assert(one('.relay-settings').textContent.includes('尚未向当前账号开放'),'experiment excluded');await capture('excluded');
    await refresh('disabled');assert(one('.relay-settings').textContent.includes('未开启'),'server disabled');await capture('disabled');
    await refresh('offline');assert(one('.relay-error'),'offline message');await capture('offline');
    await refresh('rejected');click(button('继续播放'));await sleep(150);await wait(ready,'rejected pull');assert(one('.relay-operation').textContent.includes('拒绝'),'rejected outcome');await capture('rejected');
    await refresh('lost');click(button('继续播放'));await sleep(150);await wait(ready,'lost pull');assert(all('.relay-operation').some(n=>n.textContent.includes('未自动重试')),'lost response not explained');await capture('lost_pull');
    await refresh('permission');click(button('继续播放'));await sleep(150);await wait(()=>ready()&&button('重新准备播放'),'permission failure');await capture('permission_failure');
    await emit('fixture',{mode:'song'});click(button('重新准备播放'));await sleep(150);await wait(()=>ready()&&state()==='playing'&&position()>0.2,'music playback');assert(position()<10,'song did not restart');await capture('music_playing',true);click(one('[aria-label="停止播放"]'));
    await refresh('voice');await capture('voice_offer');click(button('继续播放'));await sleep(150);await wait(()=>ready()&&state()==='playing'&&position()>=12.5,'voice resume');await capture('voice_playing',true);click(one('[aria-label="停止播放"]'));
    await refresh('song');click(one('input[name="personalServiceSetting"][value="2"]'));await wait(()=>one('dialog[open]')&&!one('.relay-prompt'),'modal suppression');await capture('modal_suppression');click(all('dialog[open] button').find(n=>n.textContent.trim()==='取消'));await wait(()=>!one('dialog[open]')&&one('.relay-prompt'),'prompt restored');await capture('modal_restored');
    click(one('[aria-label="关闭接续提示"]'));await wait(()=>!one('.relay-prompt'),'close prompt');assert(document.activeElement===one('[aria-label="打开设置"]'),'close focus did not return');await capture('closed_focus');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,syntheticQueueApplications:2,playbackVerified:true,decoder:'libmpv',audioOutput:'null',mediaSubstitution:'exact synthetic CDN URL to generated PCM',autoDismiss:true});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
