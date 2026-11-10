(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<150;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=node=>{assert(node&&!node.disabled,`unavailable ${node?.outerHTML}`);node.scrollIntoView({block:'nearest'});node.focus({preventScroll:true});node.click();};
  const input=(node,value)=>{node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));};
  const ready=()=>one('.history-page')?.getAttribute('aria-busy')==='false';
  let stageCount=0;
  async function capture(name,extra={}){
    await sleep(400);
    // A history read can finish while the entering route is still translated.
    // Check clipping after the real transition (including its no-frame deadline).
    const settledHistory=()=>one('.page-view:not(.page-enter-active,.page-leave-active) .history-page');
    await wait(settledHistory,`history route did not settle: ${name}`);
    assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
    const page=settledHistory().getBoundingClientRect(),main=one('.main-content').getBoundingClientRect();
    const note=one('.history-note').getBoundingClientRect();
    assert(page.bottom<=main.bottom+1,`history page clipped: ${name} (${page.bottom} > ${main.bottom})`);
    assert(note.bottom<=main.bottom+1,`history footer clipped: ${name} (${note.bottom} > ${main.bottom})`);
    const rows=all('.history-window .history-row');assert(rows.length<=35,'unbounded history rows');
    for(const row of rows){for(const action of row.querySelectorAll('.history-actions button')){
      const b=action.getBoundingClientRect(),cell=action.parentElement.getBoundingClientRect();
      assert(b.left>=cell.left-1&&b.right<=cell.right+1,'action outside cell');
    }}
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,rows:rows.length,total:one('.history-table')?.getAttribute('aria-rowcount'),pageBottom:page.bottom,mainBottom:main.bottom,footerBottom:note.bottom,scope:one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim(),dialog:!!one('dialog[open]'),...extra});await sleep(900);
  }
  async function theme(value){click(one('[aria-label="外观主题"]'));await wait(()=>button(value),'theme menu');click(button(value));await wait(()=>document.documentElement.dataset.theme===(value==='深色'?'dark':'light'),'theme changed');}
  async function tab(label){click(all('[aria-label="播放记录类型"] button').find(n=>n.textContent.trim()===label));await wait(()=>ready()&&one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim()===label,'history tab '+label);await sleep(250);}
  async function removeFirst(){click(one('.history-actions [aria-label^="删除"]'));await wait(()=>one('dialog[open]'),'delete confirmation');click(button('确认'));await wait(()=>!one('dialog[open]')&&ready(),'delete settled');}
  try{
    await wait(()=>button('最近播放')&&one('.track-row'),'app initialized');await theme('浅色');
    click(button('最近播放'));await wait(()=>ready()&&one('.history-row[aria-rowindex]'),'history loaded');await capture('songs_light');
    const scroller=one('.history-scroll');scroller.scrollTop=scroller.scrollHeight;await sleep(350);
    assert(one('.history-row[aria-rowindex="301"]'),'last virtual row');await capture('songs_scrolled');scroller.scrollTop=0;await sleep(350);
    input(one('[aria-label="筛选播放记录"]'),'台湾 a');await wait(()=>one('.history-table')?.getAttribute('aria-rowcount')==='2','normalized filtering');await capture('filtered');
    input(one('[aria-label="筛选播放记录"]'),'no-match');await wait(()=>one('.history-content .empty-state'),'no result');await capture('filter_empty');click(one('[aria-label="清空历史筛选"]'));await wait(()=>one('.history-row[aria-rowindex]'),'clear filter');
    const remove=one('.history-actions [aria-label^="删除"]');click(remove);await wait(()=>one('dialog[open]'),'confirmation');await capture('delete_cancel');click(button('取消'));await wait(()=>!one('dialog[open]'),'modal closed');assert(document.activeElement===remove,'focus restored after modal');
    for(const name of ['播客','有声书','声音','歌单','专辑']){await tab(name);await capture('type_'+({播客:'podcast',有声书:'book',声音:'voice',歌单:'playlist',专辑:'album'}[name]));}
    await theme('深色');await capture('album_dark');await tab('歌曲');await capture('songs_dark');
    // Every write below targets the synthetic in-memory backend, never a profile.
    await emit('fixture',{mode:'lost'});await removeFirst();await wait(()=>one('.history-recovery'),'unresolved outcome');await capture('lost_reply');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small pending');await capture('lost_small');
    await emit('fixture',{mode:'offline'});click(button('核对多端记录'));await wait(()=>ready()&&one('.catalog-error'),'pending read error');await capture('lost_offline_small');
    await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120,'restore size');
    assert(button('清空记录').disabled,'pending mutation blocks new deletion');
    await emit('fixture',{mode:'normal'});click(button('核对多端记录'));await wait(()=>button('保留当前状态')&&!button('保留当前状态').disabled,'readback');await capture('reconciled');
    click(button('保留当前状态'));await wait(()=>one('dialog[open]'),'accept confirmation');await capture('accept_confirmation');click(button('确认'));await wait(()=>!one('.history-recovery')&&!one('dialog[open]'),'accepted state');
    await emit('fixture',{mode:'rejected'});await removeFirst();await wait(()=>one('.history-outcome')?.textContent.includes('未完成'),'definite rejection');await capture('rejected');
    await emit('fixture',{mode:'offline'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&one('.catalog-error'),'cached error');await capture('offline_cached');
    await emit('fixture',{mode:'normal'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&!one('.catalog-error'),'read recovered');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark');
    const nav=one('.sidebar-scroll');for(const item of all('.sidebar .nav-item')){item.scrollIntoView({block:'nearest'});await sleep(15);const a=item.getBoundingClientRect(),b=nav.getBoundingClientRect();assert(a.top>=b.top-1&&a.bottom<=b.bottom+1,'sidebar item unreachable at minimum height');}nav.scrollTop=0;await capture('small_navigation');
    await theme('浅色');await capture('small_light');await tab('有声书');await capture('book_small');
    await emit('fixture',{mode:'empty'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&one('.history-content .empty-state'),'empty history');await capture('empty');
    await emit('fixture',{mode:'normal'});
    const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
    const {epoch}=await invoke('account_snapshot');
    for(const [label,kind] of [['歌曲','catalog'],['声音','voice']]){
      await tab(label);
      const play=one('.history-play-selected'),add=one('.history-add-selected');
      assert(play?.disabled&&add?.disabled,'no history selection');
      for(const id of [3,2])click(one(`input[aria-label="选择 合成记录 ${id}"]`));
      await wait(()=>!play.disabled,"selected history action enabled");
      click(play);
      let player;
      for(let i=0;i<120;i++){
        const state=await invoke('player_snapshot',{epoch});
        if(state.resourceKind===kind&&(state.track?.voiceId??state.track?.id)==='2'&&state.state==='playing'&&state.position>.15){player=state;break;}
        await sleep(100);
      }
      assert(player,'selected history '+kind+' did not advance');
      const queue=await invoke('player_queue',{epoch});
      const batchTracks=queue.tracks.map(row=>row.track.voiceId??row.track.id);
      assert(batchTracks.join(',')==='2,3','selected history queue follows displayed order');
      const held=await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
      assert(held.state==='paused'&&held.playId===player.playId,'selected history incumbent paused');
      // Playback refreshes native local history and can replace a remote title
      // on the same ID. Resolve the new visible labels from the post-pause
      // receipt, then wait for that projection instead of reusing old rows.
      const historyKind=kind==='catalog'?'song':'voice';
      const {value:latest}=await invoke('history_page',{epoch,kind:historyKind,refresh:false});
      const rowLabel=id=>{
        const row=latest.entries.find(entry=>entry.id===String(id));
        assert(row?.resource.kind===historyKind,`current ${historyKind} history row ${id}`);
        return `选择 ${historyKind==='song'?row.resource.track.name:row.resource.voice.name}`;
      };
      const selectedLabels=new Map([2,4].map(id=>[id,rowLabel(id)]));
      const check=id=>all(`[aria-label="${label}播放记录"] .history-window input[type=checkbox]`)
        .find(row=>row.getAttribute('aria-label')===selectedLabels.get(id));
      input(one('[aria-label="筛选播放记录"]'),'合成');
      await wait(()=>ready()&&add.disabled&&check(2)&&check(4)&&!check(2).checked&&!check(4).checked,
        'post-pause history projection and selection reset');
      for(const id of [4,2])click(check(id));
      await wait(()=>!add.disabled,'selected history add enabled');
      click(add);
      const feedback=()=>one('.history-add-notice,.history-add-error');
      await wait(feedback,'selected history add completed');
      assert(feedback().textContent.trim()==='已添加到播放列表',
        `selected history add: ${feedback().textContent.trim()}`);
      const after=await invoke('player_snapshot',{epoch});
      const addedQueue=await invoke('player_queue',{epoch});
      const addedTracks=addedQueue.tracks.map(row=>row.track.voiceId??row.track.id);
      assert(addedTracks.join(',')==='2,4,3','selected history add inserts after current');
      assert(after.state==='paused'&&after.playId===held.playId&&after.entryId===held.entryId&&
        after.position===held.position,'selected history add preserves paused transport');
      await capture('selected_'+kind+'_playback',{batchTracks,playbackKind:player.resourceKind,
        addedTracks,addKeptPausedTransport:true});
    }
    await invoke('player_action',{epoch,action:{kind:'stop'}});

    const pref=()=>one('input[name="recent-history-sync"]');
    const preference=async()=> (await invoke('history_sync_preference',{epoch,action:{kind:'read'}})).value;
    const prefDialog=()=>all('dialog[open]').find(d=>d.textContent.includes('播放列表同步'));
    const showPreferences=async()=>{
      click(one('[aria-label="打开设置"]'));
      await wait(()=>pref()&&!pref().disabled,'recent-play preference ready');
      pref().scrollIntoView({block:'center'});
      await sleep(350);
    };
    const capturePreference=async(name)=>{
      await sleep(300);
      const scroll=one('.settings-scroll').getBoundingClientRect(), row=pref().getBoundingClientRect();
      assert(row.top>=scroll.top-1&&row.bottom<=scroll.bottom+1,'recent-play preference clipped');
      assert(document.documentElement.scrollWidth<=innerWidth,'settings horizontal overflow');
      await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{preferenceUi:true,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,checked:pref().checked,dialog:!!prefDialog()});
      await sleep(900);
    };
    const openSongHistory=async()=>{
      click(button('最近播放'));
      const activeTab=()=>one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim();
      await wait(()=>ready()&&activeTab(),'history reopened');
      if(activeTab()!=='歌曲')await tab('歌曲');
    };
    const currentSongs=async()=> (await invoke('history_page',{epoch,kind:'song',refresh:false})).value;
    await showPreferences();
    assert(pref().checked&&!(await preference()).firstDisableConfirmed,'initial history policy');
    click(pref());
    await wait(prefDialog,'first-off confirmation');
    assert(prefDialog().textContent.includes('关闭后，本设备最近播放列表仅展示本地记录'),'official confirmation copy');
    assert(pref().checked&&(await preference()).enabled,'no disable before confirmation');
    await capturePreference('sync_first_off_confirm');
    click(prefDialog().querySelector('[aria-label="关闭设置确认"]'));
    await wait(()=>!prefDialog()&&document.activeElement===pref(),'return focus after closing confirmation');
    assert(pref().checked&&!(await preference()).firstDisableConfirmed,'cancellation does not consume confirmation');
    click(pref());
    await wait(prefDialog,'first-off confirmation reopens');
    click(button('确定关闭'));
    await wait(()=>!prefDialog()&&pref()&&!pref().checked&&!pref().disabled,'confirmed local-only policy');
    assert((await preference()).firstDisableConfirmed,'confirmation persisted');
    await capturePreference('sync_confirmed_off');
    await openSongHistory();
    const localOnly=await currentSongs();
    assert(localOnly.entries.length>0&&localOnly.entries.every(row=>row.origin==='local')&&!localOnly.remoteIncluded,'history displays only local entries');
    await capture('sync_local_history');
    await showPreferences();
    assert(!pref().checked,'reopening settings retains disabled state');
    click(pref());
    await wait(()=>pref().checked&&!pref().disabled,'history policy re-enabled');
    click(pref());
    await wait(()=>!pref().checked&&!pref().disabled,'second disable skips confirmation');
    assert(!prefDialog(),'confirmation only once');
    click(pref());
    await wait(()=>pref().checked&&!pref().disabled,'second re-enable');
    await theme('深色');
    pref().scrollIntoView({block:'center'});
    await capturePreference('sync_enabled_again');
    await openSongHistory();
    await wait(()=>one('.history-status input')?.checked&&ready(),'synced history ready');
    const combined=await currentSongs();
    assert(combined.remoteIncluded&&combined.entries.some(row=>row.origin==='remote')&&combined.entries.some(row=>row.origin==='local'),'re-enabled history combines retained rows');
    await capture('sync_combined_history');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,playbackVerified:true});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
