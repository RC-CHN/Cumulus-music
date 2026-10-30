// Actual Tauri/WebView: native cursor hover, production card controls and IPC.
(async () => {
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const assert = (v, m) => { if (!v) throw Error(m); };
  const wait = async (f, m) => { for (let i=0;i<220;i++) { if (f()) return; await sleep(60); } throw Error(m); };
  const button = (name, root=document) => [...root.querySelectorAll('button')].find(b => b.getAttribute('aria-label')===name || b.textContent.trim()===name);
  const click = n => { assert(n && !n.disabled, 'missing or disabled control'); n.focus({preventScroll:true}); n.click(); };
  const emit = (stage, data={}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'ux-proof', payload:{stage,...data}});
  const fixture = async mode => { window.__searchFixtureAck=false; await emit('search-fixture',{mode}); await wait(()=>window.__searchFixtureAck,'fixture ack'); };
  const pointer = async (x,y) => { window.__searchPointerAck=false; await emit('search-pointer',{x,y}); await wait(()=>window.__searchPointerAck,'native pointer ack'); };
  const invoke = (command,args={}) => window.__TAURI_INTERNALS__.invoke(command,args);
  const imageSrc=Object.getOwnPropertyDescriptor(HTMLImageElement.prototype,'src');
  let localImageAssignments=0;
  Object.defineProperty(HTMLImageElement.prototype,'src',{...imageSrc,set(value){
    if(String(value).includes('/cumulus-ui-test-')) {
      localImageAssignments++;
      value=String(value).includes('cover_error') ? 'data:image/png;base64,aW52YWxpZA==' : 'data:image/svg+xml,'+encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320"><rect width="320" height="320" fill="#5f458c"/><circle cx="160" cy="145" r="85" fill="#e3a6ba"/><path d="M115 220V110l90-18v102" fill="none" stroke="#fff" stroke-width="12"/></svg>');
    }
    imageSrc.set.call(this,value);
  }});
  let count=0, step='start';
  const ipcCalls=[];const originalFetch=window.fetch;
  window.fetch=async function(url,options){
    const command=String(url).split('/').at(-1);
    if(['catalog_page','play_selection','player_action'].includes(command)) {const body=JSON.parse(options.body);ipcCalls.push({command,query:body.query,selection:body.selection,action:body.action});}
    return originalFetch.call(window,url,options);
  };
  const ready=()=>one('#search-result-panel')?.getAttribute('aria-busy')==='false' && !one('.search-results-leave-active');
  async function choose(category) {
    click(one(`[data-category="${category}"]`));
    await sleep(0);await wait(()=>one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true' && ready() && one('.search-result-card') && !one('.search-results-enter-active'),'category '+category); await sleep(180);
  }
  async function hover(card) {
    document.activeElement?.blur(); await pointer(innerWidth-3, 75); await sleep(200);
    const r=card.querySelector('.search-result-art').getBoundingClientRect();
    await pointer(r.left+r.width*.4,r.top+r.height*.4);
    await wait(()=>card.matches(':hover'),'native hover hit'); await sleep(230);
    const play=card.querySelector('.search-card-play');
    assert(!play || Number(getComputedStyle(play).opacity)>0.99,'hover reveals play');
    if(play){const b=play.getBoundingClientRect(); assert(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2)?.closest('button')===play,'play hit target');}
  }
  async function capture(label) {
    step=label; await wait(()=>all('dialog,.search-result-panel,.action-menu').every(n=>n.getAnimations().every(a=>a.playState!=='running'&&!a.pending)),'animations settled');await sleep(140);
    const panel=one('.search-result-panel');
    assert(document.documentElement.scrollWidth<=innerWidth && (!panel || panel.scrollWidth<=panel.clientWidth+1),'horizontal containment');
    assert(!one('button button'),'no nested button');
    const menu=one('.action-menu'); if(menu){const r=menu.getBoundingClientRect();assert(r.left>=0&&r.top>=0&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1,'menu contained');}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,label,nativePointer:true});await sleep(420);
  }
  const submit=()=>{const input=one('[aria-label="搜索音乐"]');input.value='合成卡片';input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));};
  try {
    await wait(()=>one('.track-row'),'daily ready');
    const input=one('[aria-label="搜索音乐"]');input.value='合成卡片';input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(()=>one('.search-results-page')&&ready(),'results ready');
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]){
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'resize');
      click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(t==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu')&&document.documentElement.dataset.theme===t,'theme');
      submit();await sleep(0);await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','fresh theme search');
      for(const category of ['playlists','albums','artists','podcasts','voices']){
        await choose(category);const card=one('.search-result-card');await hover(card);await capture(`${category}_hover_${w}_${t}`);
        const cr=card.getBoundingClientRect();card.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:cr.left+80,clientY:cr.top+60}));await wait(()=>one('.action-menu'),'context menu');await capture(`${category}_menu_${w}_${t}`);
        document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));await wait(()=>!one('.action-menu'),'menu close');assert(card.contains(document.activeElement),'menu restores card focus');
        const keyboardTarget=card.querySelector('.search-card-play');keyboardTarget.focus({preventScroll:true});
        keyboardTarget.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));await wait(()=>one('.action-menu'),'keyboard context menu');
        assert(one('.action-menu').contains(document.activeElement),'keyboard menu takes focus');
        document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));await wait(()=>!one('.action-menu'),'keyboard menu close');assert(document.activeElement===keyboardTarget,'keyboard menu restores exact target');
        if(category==='voices'){
          const before=ipcCalls.length;card.querySelector('.menu-trigger').dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));await sleep(100);
          assert(ipcCalls.length===before,'double click on menu must not play the voice');
        }
        if(w===1120&&t==='light'){
          click(card.querySelector('.search-card-play'));await wait(()=>card.classList.contains('search-card-current')&&card.querySelector('.search-card-play').getAttribute('aria-label').startsWith('暂停'),'card playback '+category);assert(all('.search-card-current').length===1,'only matching card is active');await capture(`${category}_playing`);
          click(card.querySelector('.search-card-play'));await wait(()=>card.querySelector('.search-card-play').getAttribute('aria-label').startsWith('播放')&&button('播放'),'card paused');await capture(`${category}_paused`);
          const epoch=(await invoke('account_snapshot')).epoch;
          await invoke('player_action',{epoch,action:{kind:'stop'}});
        }
      }
    }
    // Voice double click uses the selected episode while its radio link stays independent.
    await choose('voices');const voice=all('.search-result-card')[1];voice.dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));await wait(()=>voice.classList.contains('search-card-current')&&voice.querySelector('.search-card-play').getAttribute('aria-label').startsWith('暂停'),'voice double click');await capture('voice_double_play');
    click(voice.querySelector('.menu-trigger'));await wait(()=>one('.action-menu'),'voice menu');click(button('下载声音'));await wait(()=>one('.download-dialog[open]'),'voice download options');await capture('voice_download_options');
    const close=one('.download-dialog button[aria-label^="关闭"]');click(close);await wait(()=>!one('.download-dialog'),'download cancel');
    step='playback_read_error';await fixture('card_error');submit();await sleep(0);await choose('albums');click(one('.search-card-play'));await wait(()=>one('.playback-message')||document.body.innerText.includes('请重试')||button('重试'),'read failure feedback');await capture('playback_read_error');
    await fixture('good');click(button('重试'));await wait(()=>one('.search-card-current')?.querySelector('.search-card-play')?.getAttribute('aria-label').startsWith('暂停'),'playback retry');await capture('playback_recovered');
    await fixture('cover');submit();await sleep(0);await choose('playlists');await wait(()=>one('.search-result-art img')?.complete&&one('.search-result-art img').naturalWidth>0,'local artwork loaded');await hover(one('.search-result-card'));await capture('cover_loaded');
    await fixture('cover_error');submit();await sleep(0);await choose('artists');await choose('playlists');await wait(()=>!one('.search-result-art img'),'failed cover fallback');assert(one('.search-result-art > .icon'),'placeholder after error');await hover(one('.search-result-card'));await capture('cover_failed');
    assert(localImageAssignments>0,'image outcome injection exercised');
    await emit('complete',{ok:true,stageCount:count,nativePointer:true,coverOutcomeInjected:true,keyboardContextMenu:true,voiceMenuDoubleClickSafe:true,realMusicRequests:0,realAccountWrites:0});
  }catch(e){await emit('failed',{step,message:String(e),stack:e?.stack,stageCount:count,ipcCalls:ipcCalls.slice(-18),body:document.body.innerText.slice(0,4000)});}
})();
