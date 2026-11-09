// Actual Recently Played tabs and guarded chapter clicks with sealed native sources.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  let phase = 'startup', count = 0, last;
  async function wait(observe, label) {
    phase = label;
    const deadline = performance.now() + 15_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleep(60);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(node?.isConnected && !node.matches(':disabled,[aria-disabled="true"]'), 'enabled proof control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const cases = [
    { kind: 'podcast', tab: '播客', id: '81', radio: '合成历史播客', title: '播客第二集',
      voices: ['811', '812'], media: ['1811', '1812'] },
    { kind: 'audiobook', tab: '有声书', id: '82', radio: '合成历史有声书', title: '有声书第二章',
      voices: ['821', '822'], media: ['1821', '1822'] },
  ];
  const rowFor = (title) => all('.history-window .history-row').find(
    (row) => one('.history-name', row)?.textContent.trim() === title,
  );
  const tabs = '[aria-label="播放记录类型"]';
  const activeTab = () => one(`${tabs} [aria-current]`)?.textContent.trim();
  async function capture(name, evidence) {
    await wait(() => !one('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)'),
      name + ' tab animation settled');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const row = rowFor(evidence.radio);
    const label = one('.history-chapter', row);
    const bounds = (evidence.playing ? one('.history-name', row) : label)?.getBoundingClientRect();
    assert(bounds?.width > 0 && bounds.height > 0 && bounds.bottom <= innerHeight
      && bounds.left >= 0 && bounds.right <= innerWidth, 'visible history row');
    assert(document.documentElement.scrollWidth <= innerWidth, 'history page fits viewport');
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      ...evidence, subtitle: label?.textContent.replace(/\s+/g, '') ?? null,
    });
  }
  try {
    const { epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'isolated account restored');
    const snapshot = () => invoke('player_snapshot', { epoch });
    const action = (action) => invoke('player_action', { epoch, action });
    const history = (kind) => invoke('history_page', { epoch, kind, refresh: false });
    const historyButton = await wait(
      () => all('.sidebar button:not(:disabled)').find((node) => node.textContent.trim() === '最近播放'),
      'history sidebar control',
    );
    click(historyButton);
    await wait(() => one(tabs), 'history tabs mounted');
    // Avoid optional next-source prefetch: any source request must belong to the clicked chapter.
    await action({ kind: 'mode', mode: 'single_loop' });
    for (const item of cases) {
      click(all(`${tabs} button`).find((node) => node.textContent.trim() === item.tab));
      const chapterButton = await wait(() => {
        const row = rowFor(item.radio);
        const button = one('.history-chapter button', row);
        return activeTab() === item.tab && one('.history-page')?.getAttribute('aria-busy') === 'false'
          && button?.textContent.trim() === item.title && !button.disabled && button;
      }, item.kind + ' chapter loaded');
      assert(chapterButton.getAttribute('aria-label') === '播放章节 ' + item.title, 'named chapter control');
      const page = await history(item.kind);
      assert(page.epoch === epoch && page.value.remoteIncluded && page.value.kind === item.kind, 'native page scope');
      const chapter = page.value.entries.find((entry) => entry.id === item.id)?.chapter;
      assert(chapter?.name === item.title && chapter.tag === '续听' && chapter.playable
        && !chapter.recent && chapter.percent === null && !chapter.completed, 'remote native chapter projection');
      assert(Object.keys(chapter).sort().join(',') === 'completed,name,percent,playable,recent,tag',
        'chapter DTO has no raw episode ID, media ID or source');
      assert(!one('.history-chapter-progress', rowFor(item.radio)), 'no invented progress');
      let staticChapter = null;
      if (item.kind === 'podcast') {
        const staticRow = rowFor('合成仅文本播客');
        const entry = page.value.entries.find((entry) => entry.id === '83');
        assert(entry?.chapter?.name === '只有标题的单集' && entry.chapter.playable === false
          && one('.history-chapter-name', staticRow)?.textContent.trim() === entry.chapter.name
          && !one('.history-chapter button', staticRow), 'name-only chapter remains readable without a play target');
        staticChapter = entry.chapter.name;
      }
      const before = await snapshot();
      await capture(item.kind + '_subtitle', {
        radio: item.radio, chapter, staticChapter, previousPlayId: before.playId, playing: false,
      });

      click(chapterButton);
      const state = await wait(async () => {
        const candidate = await snapshot();
        last = { state: candidate.state, voiceId: candidate.track?.voiceId, trackId: candidate.track?.id,
          position: candidate.position, error: candidate.error };
        return candidate.state === 'playing' && candidate.resourceKind === 'voice'
          && candidate.playId !== before.playId && candidate.track?.voiceId === item.voices[1]
          && candidate.track?.id === item.media[1] && candidate.position > .2 && !candidate.error && candidate;
      }, item.kind + ' exact chapter drives native null playback');
      const queue = await invoke('player_queue', { epoch });
      const voices = queue.tracks.map((row) => row.track.voiceId);
      const media = queue.tracks.map((row) => row.track.id);
      assert(voices.join(',') === item.voices.join(',') && media.join(',') === item.media.join(','),
        'native queue retains the full radio in wire order');
      assert(queue.current === state.entryId && queue.revision === state.queueRevision && state.queueLen === 2,
        'native selected queue entry matches the playing voice');
      assert(activeTab() === item.tab, 'chapter click keeps the history tab');
      // The fixture's newly played local radio metadata has no chapter
      // fields. Official history chooses that newer row as a whole.
      await wait(async () => {
        const row = (await history(item.kind)).value.entries.find((entry) => entry.id === item.id);
        return row?.origin === 'local' && !row.chapter
          && !one('.history-chapter', rowFor(item.radio));
      }, item.kind + ' newer local history replaces the remote subtitle');
      await capture(item.kind + '_play', {
        radio: item.radio, chapter, staticChapter, previousPlayId: before.playId,
        voices, media, entryId: state.entryId, playId: state.playId, playing: true,
      });
    }
    await emit('complete', { ok: true, historyRadioChapterVerified: true, stageCount: count });
  } catch (error) {
    await emit('failed', { ok: false, stageCount: count, phase, last,
      error: error instanceof Error ? error.message : error?.code ?? error?.kind ?? String(error) });
  }
})();
