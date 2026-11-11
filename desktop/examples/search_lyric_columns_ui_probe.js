// Recovered Lyrics table column behavior, measured inside the production WebView.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event:'ux-proof', payload:{ stage, ...data } });
  const pane = () => one('#search-result-panel');
  const table = () => one('.search-lyric-list', pane());
  const header = () => one('.track-header', table());
  const row = () => one('[data-lyric-index="0"] .track-row', table());
  const resizeHandle = (name) => one(`[aria-label="调整${name}列宽"]`, header());
  const title = () => resizeHandle('标题'), album = () => resizeHandle('专辑');
  const dimensions = (element) => [...(element?.children ?? [])].map((node) => {
    const box = node.getBoundingClientRect();
    return [box.left, box.width];
  });
  const close = (a, b) => Math.abs(a-b) < 1.3;
  const near = (left, right) => left?.length === right?.length && left.every((n,i) => close(n,right[i]));
  const widths = () => dimensions(header()).map(([, width]) => width);
  const enabled = (node) => node?.getAttribute('aria-disabled') === 'false' && node.getClientRects().length;
  const value = (node, kind) => Number(node.getAttribute('aria-value'+kind));
  const key = (node, key) => node.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles:true, cancelable:true }));
  const pointer = (target, type, x, y) => target.dispatchEvent(new PointerEvent(type, { clientX:x,clientY:y,
    pointerId:49,pointerType:'mouse',isPrimary:true,button:0,buttons:type==='pointerup'?0:1,bubbles:true,cancelable:true }));
  const selected = () => row()?.getAttribute('aria-selected') === 'true';
  const expanded = () => one('[data-lyric-index="0"] [data-result-action="expand"]',table())?.getAttribute('aria-expanded') === 'true';
  const settled = () => !one('.page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active');
  let step = 'startup', count = 0, epoch, baseline;
  async function wait(check, label) {
    step = label;
    const end = performance.now()+14_000;
    do { const result = await check(); if (result) return result; await delay(50); } while (performance.now() < end);
    throw Error(label);
  }
  async function viewport(width, height) {
    await emit('resize',{width,height});
    await wait(() => innerWidth===width && innerHeight===height, 'viewport '+width);
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, 'enabled control');
    node.focus({preventScroll:true}); node.click();
  }
  async function choose(kind) {
    click(one(`[data-category="${kind}"]`));
    await wait(() => one(`[data-category="${kind}"]`)?.getAttribute('aria-selected') === 'true'
      && pane()?.getAttribute('aria-busy') === 'false' && settled(), 'settled category '+kind);
  }
  function wideAligned() {
    const heads=dimensions(header()), cells=dimensions(row());
    return heads.length===5 && cells.length===5 && heads.every((head,i)=>near(head,cells[i]));
  }
  async function capture(label, wide = true) {
    step = label;
    if (wide) {
      await wait(() => wideAligned() && enabled(title()) && enabled(album()), 'aligned wide header and rows');
      assert(all('.table-column-resizer',header()).length===2 && !one('[aria-sort]',header()),
        'only the title and album handles, no sorting');
      const [index,titleWidth,albumWidth,likes,duration]=widths();
      assert(close(index,45) && close(likes,60) && titleWidth>=179 && albumWidth>=114 && duration>=63,
        'fixed columns and recovered minimums');
      assert(close(titleWidth,value(title(),'now')) && close(albumWidth,value(album(),'now')),
        'accessible ranges reflect measured widths');
    }
    assert(document.documentElement.scrollWidth<=innerWidth && pane().scrollWidth<=pane().clientWidth+1,
      'page does not overflow horizontally');
    await wait(() => {
      const more = one('[data-result-action="more"]', row());
      if (!more || more.disabled) return false;
      const box = more.getBoundingClientRect();
      return box.width > 0 && box.height > 0 && [box.left + box.width / 2, box.right - 3]
        .every((x) => more.contains(document.elementFromPoint(x, box.top + box.height / 2)));
    }, 'row More center and right edge are clear of the scrollbar');
    const queue = await invoke('player_queue',{epoch});
    assert(queue.epoch===baseline.epoch && queue.revision===baseline.revision &&
      queue.tracks.length===baseline.tracks.length && queue.current===baseline.current, 'geometry does not change native playback');
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`, {
      label, columns:widths(), selected:selected(), expanded:expanded(), scroll:pane().scrollTop,
      viewport:[innerWidth,innerHeight], handles:all('.table-column-resizer',header())
        .filter(enabled).map((n)=>n.getAttribute('aria-label')),
      focused:document.activeElement?.getAttribute('aria-label') ?? null,
    });
  }
  async function drag(delta, cancel = false) {
    const node=title(), box=node.getBoundingClientRect(), x=box.left+box.width/2, y=box.top+box.height/2;
    assert(node.contains(document.elementFromPoint(x,y)), 'title separator has a real hit target');
    const initial=widths();
    pointer(node,'pointerdown',x,y);
    pointer(document,'pointermove',x+delta,y);
    await wait(() => node.classList.contains('dragging') && !near(widths(),initial), 'pointer previews another width');
    if (cancel) key(node,'Escape');
    else pointer(document,'pointerup',x+delta,y);
    await wait(() => !node.classList.contains('dragging') && (cancel === near(widths(),initial)),
      cancel ? 'Escape rolls back layout' : 'pointer commits layout');
  }

  try {
    await wait(() => one('.track-row'),'daily results');
    window.__searchFixtureAck=false;
    await emit('search-fixture',{mode:'lyrics'});
    await wait(() => window.__searchFixtureAck,'isolated lyrics fixture');
    epoch=(await invoke('account_snapshot')).epoch;
    baseline=await invoke('player_queue',{epoch});
    await viewport(1120,760);
    const field=one('[aria-label="搜索音乐"]');
    field.value='合成歌词列宽';
    field.dispatchEvent(new Event('input',{bubbles:true}));
    field.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(() => one('.search-results-page h1')?.textContent==='合成歌词列宽' && settled(),'exact search route');
    await choose('lyrics');
    await wait(() => enabled(title()) && enabled(album()) && wideAligned(),'lyric table measured');
    if (window.__lyricColumnsReopen) {
      await capture('fresh_process');
    } else {
      const start=widths(), originalTotal=start.reduce((a,b)=>a+b,0);
      await capture('wide_defaults');
      row().dispatchEvent(new MouseEvent('mousedown',{bubbles:true,button:0,buttons:1}));
      row().dispatchEvent(new MouseEvent('mouseup',{bubbles:true,button:0}));
      const expander=one('[data-lyric-index="0"] [data-result-action="expand"]',table());
      click(expander);
      await wait(() => selected() && expanded(),'ordinary row remains selected and expanded');
      await drag(36);
      const dragged=widths();
      assert(close(dragged[1],start[1]+36) && close(dragged[2],start[2]-36) &&
        close(dragged[4],start[4]),'title takes album surplus first');
      await capture('title_dragged');
      album().focus({preventScroll:true}); key(album(),'End');
      await wait(() => close(widths()[4],64) && widths()[2]>dragged[2], 'album consumes duration surplus');
      assert(close(widths().reduce((a,b)=>a+b,0),originalTotal),'same total column width');
      const committed=widths();
      await capture('album_keyboard');
      await drag(-26,true);
      assert(near(widths(),committed) && selected() && expanded(),'cancel preserves committed sizing and row state');
      await capture('cancelled');
      await choose('songs');
      assert(!table() && !one('.search-lyric-list .table-column-resizer'), 'ordinary search is unchanged');
      await choose('lyrics');
      await wait(() => near(widths(),committed) && expanded(),'same widths and expansion on category return');
      await capture('category_return');
      await viewport(760,540);
      await wait(() => !enabled(title()) && !enabled(album()), 'compact view uses existing layout');
      await capture('compact',false);
      await viewport(1120,760);
      await wait(() => near(widths(),committed) && wideAligned(),'wide layout restores committed widths');
      await capture('wide_restored');
    }
    await emit('complete',{ok:true,stageCount:count,columns:widths(),realMusicRequests:0,realAccountWrites:0});
  } catch(error) {
    await emit('failed',{step,message:String(error),stack:error.stack,stageCount:count,
      widths:widths(),bodyWidths:dimensions(row()),width:innerWidth,selected:selected(),expanded:expanded()});
  }
})();
