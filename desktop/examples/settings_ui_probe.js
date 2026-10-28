(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<150;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=node=>{assert(node&&!node.matches(':disabled'),`unavailable ${node?.outerHTML}`);node.scrollIntoView({block:'nearest'});node.focus({preventScroll:true});node.click();};
  const ready=()=>one('.settings-page')?.getAttribute('aria-busy')==='false';
  const radio=(key,value)=>all(`input[name="${key}"]`).find(n=>n.value===String(value));
  const check=key=>one(`input[name="${key}"]`);
  let stageCount=0;
  const focusChecks=[];
  const label=node=>node?.getAttribute('aria-label')||node?.getAttribute('name')||node?.tagName||'missing';
  const focus=(node,reason)=>focusChecks.push({reason,expected:label(node),actual:label(document.activeElement),passed:document.activeElement===node});
  async function capture(name){
    await sleep(350);
    assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
    const page=one('.settings-page').getBoundingClientRect(),main=one('.main-content').getBoundingClientRect(),scroll=one('.settings-scroll').getBoundingClientRect();
    assert(page.bottom<=main.bottom+1&&scroll.bottom<=main.bottom+1,'settings clipped by player');
    assert(!document.body.textContent.includes('SYNTHETIC_PRIVATE'),'raw native field leaked');
    const top=one('.topbar').getBoundingClientRect();
    const gear=one('[aria-label="打开设置"]').getBoundingClientRect();assert(gear.left>=top.left&&gear.right<=top.right,'settings entry clipped');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,pageBottom:page.bottom,mainBottom:main.bottom,scrollBottom:scroll.bottom,dialog:!!one('dialog[open]'),recovery:all('.settings-recovery').length,checked:all('.privacy-options input:checked').map(n=>({name:n.name,value:n.value}))});await sleep(900);
  }
  async function theme(value){click(radio('settings-theme',value));await wait(()=>document.documentElement.dataset.theme===(value==='system'?(matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light'):value),'theme selection');}
  async function refresh(){click(one('[aria-label="重新读取账号设置"]'));await sleep(100);await wait(ready,'settings refreshed');}
  async function change(key,value,other){const previous=all(`input[name="${key}"]:checked`).map(n=>n.value);const target=typeof value==='boolean'?check(key):radio(key,value);click(target);other?.focus({preventScroll:true});await sleep(80);assert(!ready(),'mutation never entered pending state');assert(JSON.stringify(all(`input[name="${key}"]:checked`).map(n=>n.value))===JSON.stringify(previous),'option optimistically changed before readback');await wait(ready,'setting settled');focus(other??(target.matches(':disabled')?one('[aria-label="重新读取账号设置"]'):target),`change ${key}`);}
  try{
    await wait(()=>one('.track-row')&&one('[aria-label="打开设置"]')&&!one('.account-button').disabled,'initialized');await sleep(300);click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&radio('shareSetting',0),'settings loaded');
    await theme('light');await capture('appearance_light');await theme('dark');await capture('appearance_dark');await theme('system');await capture('appearance_system');await theme('light');
    radio('shareSetting',0).scrollIntoView({block:'center'});await capture('privacy_light');
    await change('shareSetting',1);assert(radio('shareSetting',1).checked,'share readback not selected');await capture('share_confirmed');
    await change('playRecordSetting',2,radio('settings-theme','light'));assert(radio('playRecordSetting',2).checked,'privacy scope not updated');click(one('.settings-linked summary'));assert(all('.settings-linked dd').every(n=>n.textContent==='关闭'),'coupled privacy not updated');await capture('linked_privacy');
    const personal=radio('personalServiceSetting',2);click(personal);await wait(()=>one('dialog[open]'),'personal confirmation');assert(radio('personalServiceSetting',0).checked,'confirmation changed setting');await capture('personal_cancel');click(button('取消'));await wait(()=>!one('dialog[open]'),'personal cancel');assert(document.activeElement===personal,'confirmation focus did not return');
    click(personal);await wait(()=>one('dialog[open]'),'personal confirmation again');click(button('确认'));await wait(()=>ready()&&!one('dialog[open]'),'personal saved');assert(personal.checked,'personal change not read back');await capture('personal_saved');
    await emit('fixture',{mode:'unapplied'});await change('allowLikedNotify',false);await wait(()=>one('.settings-recovery'),'lost unapplied intent');assert(check('allowLikedNotify').checked,'failed write changed displayed value');await capture('lost_unapplied');
    await emit('fixture',{mode:'offline'});click(button('核对服务端状态'));await wait(()=>ready()&&one('.catalog-error'),'reconcile offline');await capture('reconcile_offline');
    await emit('fixture',{mode:'normal'});click(button('核对服务端状态'));await wait(()=>ready()&&button('保留当前设置'),'reconcile unmatched');await capture('reconcile_unmatched');
    click(button('保留当前设置'));await wait(()=>one('dialog[open]'),'accept confirmation');await capture('accept_confirmation');click(button('确认'));await wait(()=>ready()&&!one('dialog[open]')&&!one('.settings-recovery'),'accept read state');focus(one('[aria-label="重新读取账号设置"]'),'removed accept entry');await capture('accepted_current');
    await emit('fixture',{mode:'lost'});await change('allowNewFollowerNotify',false);await wait(()=>one('.settings-recovery'),'lost applied');await capture('lost_applied');await emit('fixture',{mode:'normal'});click(button('核对服务端状态'));await wait(()=>ready()&&!one('.settings-recovery'),'reconcile matched');assert(!check('allowNewFollowerNotify').checked,'confirmed readback missing');await capture('confirmed_after_reconcile');
    await emit('fixture',{mode:'rejected'});await change('allowSubscriptionNotify',false);assert(check('allowSubscriptionNotify').checked,'403 changed option');await wait(()=>one('.settings-outcome')?.textContent.includes('未接受'),'rejected result');await capture('rejected');
    await emit('fixture',{mode:'unknown'});await refresh();assert(radio('playRecordSetting',0).matches(':disabled'),'unknown server value editable');assert(!all('input[name="playRecordSetting"]:checked').length,'unknown server value defaulted');assert(check('allowLikedNotify').matches(':disabled'),'missing server value editable');await capture('unknown_fields');
    await emit('fixture',{mode:'normal'});await refresh();await theme('dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'minimum size');await capture('small_dark');
    radio('shareSetting',1).scrollIntoView({block:'start'});await capture('small_privacy_dark');
    const download=button('打开下载管理');download.scrollIntoView({block:'end'});await sleep(250);const d=download.getBoundingClientRect(),s=one('.settings-scroll').getBoundingClientRect();assert(d.top>=s.top&&d.bottom<=s.bottom+1,'bottom setting unreachable');await capture('small_bottom');
    await theme('light');await capture('small_light');
    await emit('fixture',{mode:'slow-read'});click(one('[aria-label="重新读取账号设置"]'));await sleep(80);assert(!ready(),'delayed read was not pending');
    const daily=all('.nav-item').find(n=>n.textContent.trim()==='每日推荐');click(daily);await wait(()=>!one('.settings-page')&&one('.track-row'),'navigate while read pending');await sleep(650);assert(!one('.settings-page'),'late read reopened settings');focus(daily,'pending read navigation');
    await emit('fixture',{mode:'normal'});click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&radio('shareSetting',0),'settings reopened');await capture('pending_navigation_return');
    assert(focusChecks.every(row=>row.passed),'settings keyboard focus checks failed');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,playbackVerified:false,readbackBeforeSelection:true,focusChecks});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,focusChecks});}
})();
