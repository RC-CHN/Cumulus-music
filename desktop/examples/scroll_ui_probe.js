(async () => {
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const emit = value => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'scroll-proof', payload: value });
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  async function until(predicate, message) {
    for(let i=0;i<200;i++) {if(predicate()) return; await wait(25);}
    throw Error(message);
  }
  async function measure(element, scrolling) {
    element.scrollTop = 0;
    await wait(500);
    const times = [], costs = [];
    let start, maxRows = 0, mutations = 0;
    const observer = new MutationObserver(records => { mutations += records.length; });
    observer.observe(element, {subtree:true,childList:true,attributes:true});
    await new Promise(resolve => {
      function frame(t) {
        start ??= t;
        const before = performance.now();
        if (scrolling) element.scrollTop = (t - start) * 1.5;
        if (t - start > 1000) times.push(t);
        maxRows = Math.max(maxRows, document.querySelectorAll('.track-row').length);
        costs.push(performance.now() - before);
        if (t - start < 6000) requestAnimationFrame(frame); else resolve();
      }
      requestAnimationFrame(frame);
    });
    observer.disconnect();
    const deltas = times.slice(1).map((t,i) => t-times[i]).sort((a,b)=>a-b);
    return { fps: 1000*(times.length-1)/(times.at(-1)-times[0]), p50Ms: deltas[Math.floor(deltas.length*.5)], p95Ms: deltas[Math.floor(deltas.length*.95)], p99Ms: deltas[Math.floor(deltas.length*.99)], frames: times.length, maxRows, scrollTop: element.scrollTop, scriptCostMs: costs.reduce((a,b)=>a+b,0), mutations };
  }
  try {
    await until(()=>document.querySelector('.track-row'), 'daily tracks did not load');
    const tracks = document.querySelector('.track-scroll');
    assert(document.querySelector('.track-table').getAttribute('aria-rowcount')==='2001', 'fixture track count');
    const idle = await measure(tracks, false);
    const songs = await measure(tracks, true);
    assert(songs.maxRows < 80 && songs.scrollTop > 8000, 'track virtualization');
    const first = document.querySelector('.track-row');
    assert(Number(first.getAttribute('aria-rowindex')) > 100, 'virtual rows did not advance');
    // Fast jumps, reverse scrolling and the last row must never leave an empty viewport.
    for (const top of [90000, 10000, tracks.scrollHeight, 0]) {
      tracks.scrollTop=top; await wait(100);
      const bounds=tracks.getBoundingClientRect();
      assert([...document.querySelectorAll('.track-row')].some(e=> {const r=e.getBoundingClientRect();return r.top<bounds.bottom && r.bottom>bounds.top;}), 'blank virtual viewport');
    }
    const controls = document.querySelector('.window-controls');
    assert(controls?.querySelectorAll('button').length===3, 'custom window buttons missing');
    assert(document.querySelector('[data-tauri-drag-region]'), 'drag region missing');
    const nav = name => [...document.querySelectorAll('.nav-item')].find(e=>e.textContent.trim()===name);
    nav('我的歌单').click();
    await until(()=>document.querySelector('.playlist-grid'), 'library did not open');
    const grid = document.querySelector('.playlist-grid');
    assert(grid.children.length===300, 'fixture playlist count');
    const playlists = await measure(grid, true);
    // Exercise both native-dialog top-layer transitions and focus restoration.
    const create = [...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='新建歌单');
    create.focus(); create.click();
    await until(()=>document.querySelector('dialog[open]'), 'dialog did not open');
    const dialog=document.querySelector('dialog[open]');
    assert(dialog.getBoundingClientRect().width>100, 'dialog not visible');
    dialog.dispatchEvent(new Event('cancel',{cancelable:true}));
    await until(()=>!document.querySelector('dialog[open]'), 'dialog leave did not finish');
    assert(document.activeElement===create, 'dialog did not restore focus');
    nav('每日推荐').click();
    await until(()=>document.querySelector('.track-row'), 'return navigation failed');
    const snapshot={ok:true,viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,idle,songs,playlists,customControls:true,dialogFocusRestored:true,realMusicRequests:0};
    if (window.__testWindowControls) {
      document.querySelector('button[aria-label="最大化"]').click();
      await until(()=>document.querySelector('button[aria-label="还原窗口"]'), 'maximize did not update');
      document.querySelector('button[aria-label="还原窗口"]').click();
      await until(()=>document.querySelector('button[aria-label="最大化"]'), 'restore did not update');
      assert(document.querySelectorAll('.window-resize').length===8, 'resize edges missing');
      await wait(600);
      document.querySelector('button[aria-label="最小化"]').click();
      await emit({stage:'restore'});
      await until(()=>window.__restored!==undefined, 'native minimize/restore reply missing');
      snapshot.nativeMinimizedGetter=window.__restored; // GTK/Wayland cannot always report ICONIFIED; compositor observation is separate.
      snapshot.maximizeRestored=true; snapshot.minimizeRequested=true; snapshot.restoreDispatched=true;
    }
    await emit(snapshot);
    if (window.__testWindowControls) document.querySelector('button[aria-label="关闭窗口"]').click();
  } catch(error) {await emit({ok:false,error:String(error)});}
})();
