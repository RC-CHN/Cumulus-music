// Production settings + native media host, isolated fixture account/SQLite/PCM.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, rest={}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'ux-proof',payload:{stage,...rest}});
  const wait = async (test,label) => { for(let i=0;i<150;i++){if(test())return;await sleep(100);}throw Error(label); };
  const button = text => all('button').find(b=>b.textContent.trim()===text);
  const click = node => {assert(node&&!node.disabled,'unavailable control');node.focus({preventScroll:true});node.click();};
  const box = () => one('input[name="system-media-enabled"]');
  const section = () => one('.system-media-settings');
  const phase = () => section()?.querySelector('.media-status')?.dataset.phase;
  const refresh = () => one('[aria-label="重新读取系统媒体设置"]');
  const position = () => Number(one('.playerbar')?.dataset.position);
  const player = () => one('.playerbar')?.dataset.state;
  const focusTrace = [];
  const identify = node => node ? `${node.tagName}:${node.getAttribute('name')||node.getAttribute('aria-label')||node.className}` : null;
  for(const kind of ['focusin','focusout']) document.addEventListener(kind,event=>{
    focusTrace.push({kind,target:identify(event.target),at:performance.now()});
    if(focusTrace.length>20)focusTrace.shift();
  },true);
  let count = 0;
  const visible = node => { const r=node.getBoundingClientRect();return r.width>0&&r.height>0&&r.x>=0&&r.y>=0&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1; };
  async function capture(label, extra={}) {
    section().scrollIntoView({block:'center',behavior:'instant'});await sleep(400);
    assert(scrollY===0 && document.documentElement.scrollWidth<=innerWidth,'frame overflow');
    assert(visible(refresh()),'media refresh clipped');
    if(box()) {
      assert(visible(box()),'media toggle clipped');
      const input=box().getBoundingClientRect(), text=box().nextElementSibling.getBoundingClientRect();
      assert(input.width===16&&input.height===16&&text.x-input.right>=8,'media checkbox size/label spacing');
      assert(getComputedStyle(box()).accentColor!=='auto','media checkbox must use the app theme');
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{theme:document.documentElement.dataset.theme,phase:phase(),checked:box()?.checked??null,playerState:player(),position:position(),...extra});
    await sleep(1100);
  }
  async function settings() {click(one('[aria-label="打开设置"]'));await wait(()=>section()&&phase()&&!refresh().disabled,'settings ready');}
  async function play() {
    click(button('每日推荐'));
    await wait(()=>one('.track-row .track-play')&&!one('.track-row .track-play').disabled,'native ready');
    one('.track-row').dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
    await wait(()=>player()==='playing'&&position()>.15,'native PCM');
  }
  async function toggle(enabled) {
    assert(box().checked!==enabled,'toggle must change saved setting');click(box());
    await wait(()=>box()?.checked===enabled&&!box().disabled&&phase()===(enabled?'active':'disabled'),'native setting applied');
  }
  async function fixture(kind) {
    window.__mediaFixtureAck=null;await emit('media-fixture',{kind});
    await wait(()=>window.__mediaFixtureAck===kind,'fixture injection acknowledged');
  }
  async function theme(label) {
    click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'appearance');click(button(label));
    await wait(()=>!one('.action-menu'),'theme applied');
  }
  try {
    await wait(()=>one('.track-row .track-play')&&!one('.track-row .track-play').disabled,'daily native player ready');
    if(localStorage.getItem('cumulus.media-settings-finished')) {
      await settings();assert(box()?.checked===false&&phase()==='disabled','saved disabled policy lost after restart');
      assert(document.documentElement.dataset.theme==='dark','theme restart');await capture('reopened_disabled');
      click(box());await wait(()=>box()?.checked&&!box().disabled,'reopened enable saved');
      await play();await settings();await wait(()=>phase()==='active','reopened system media active');await capture('reopened_active');
      await emit('complete',{ok:true,stageCount:count,reopened:true,realMusicRequests:0});return;
    }
    await theme('浅色');await settings();assert(box().checked&&phase()==='idle','fresh policy');await capture('idle_light');
    await play();await settings();await wait(()=>phase()==='active','native media connected');await capture('active_light');
    const before=position();await toggle(false);await sleep(400);
    assert(player()==='playing'&&position()>before+.2,'disabling system media interrupted music');await capture('disabled_playing');
    await toggle(true);await capture('enabled_again');
    await theme('深色');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark');
    await fixture('reject');click(box());
    await wait(()=>section().querySelector('.catalog-error')&&!refresh().disabled,'write rejected');
    assert(box().checked&&phase()==='active','failed save changed actual integration');
    const focusStart=performance.now();
    // Observe the final keyboard target, not just the error text appearing.
    // A disabled input can retain focus until WebKit's next render.
    await wait(()=>document.activeElement===refresh(),'write failure focus');
    await capture('save_failed',{focusRecoveryWaitMs:performance.now()-focusStart,focusTrace:[...focusTrace]});
    await fixture('allow');click(refresh());await wait(()=>!section().querySelector('.catalog-error')&&!box().disabled,'explicit refresh');await capture('read_recovered');
    await fixture('damage');click(refresh());await wait(()=>!box()&&phase()==='damaged'&&!refresh().disabled,'damaged policy');await capture('damaged');
    click(button('恢复默认系统媒体设置'));await wait(()=>box()?.checked&&!box().disabled&&phase()==='active','explicit repair');
    await wait(()=>document.activeElement===refresh(),'repair focus');await capture('repaired');
    await toggle(false);await capture('saved_disabled');
    localStorage.setItem('cumulus.media-settings-finished','1');
    await emit('complete',{ok:true,stageCount:count,realMusicRequests:0,backgroundPlayback:true});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount:count,phase:phase(),playerState:player(),focused:identify(document.activeElement),focusTrace});}
})();
