// Real settings and single-song download over the isolated native download fixture.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => {
    if (!value) throw Error(message);
  };
  async function wait(read, message, attempts = 260) {
    for (let i = 0; i < attempts; i++) {
      const value = await read();
      if (value) return value;
      await sleep(50);
    }
    throw Error(message);
  }
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const button = (name, root = document) =>
    [...(root?.querySelectorAll('button') ?? [])].find(
      (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
    );
  function click(node) {
    assert(node && !node.matches(':disabled,[aria-disabled=true]'), 'enabled control: ' + node?.outerHTML);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const managerSettingsButton = () => one('.download-navigation-actions [aria-label="下载设置"]');
  const settings = () => one('section[aria-labelledby="settings-title"]');
  const dialog = () => one('dialog.download-dialog[open]');
  const choiceGroup = () => one('fieldset[data-settings-download-quality]', settings());
  const playbackRadio = (value) =>
    one(`fieldset[data-settings-playback-quality] input[name="settings-playback-quality"][value="${value}"]`, settings());
  const choiceOrder = ['沉浸环绕声', '超清母带', '高清臻音', '高解析度无损', '无损', '极高', '标准'];
  const valueOrder = ['sky', 'jymaster', 'jyeffect', 'hires', 'lossless', 'exhigh', 'standard'];
  const qualityReads = [], qualityWrites = [];
  let epoch;
  function choices(group = choiceGroup()) {
    return [...(group?.querySelectorAll('input[type="radio"]') ?? [])].map((radio) => {
      const label = radio.closest('label');
      const name = one('.download-quality-name', label).textContent.trim();
      return { name, radio, value: radio.value, disabled: radio.matches(':disabled,[aria-disabled=true]'), checked: radio.checked,
        caption: one('small', label)?.textContent.trim() ?? '' };
    });
  }
  let lastChoiceLayout = [];
  function assertChoices(verified) {
    const rows = choices();
    assert(rows.every(({radio}) => radio.name === 'settings-download-quality') &&
      JSON.stringify(rows.map(({name}) => name)) === JSON.stringify(choiceOrder) &&
      JSON.stringify(rows.map(({value}) => value)) === JSON.stringify(valueOrder), 'official inline settings rows');
    assert(rows.slice(0, 2).every((row) => row.disabled && row.caption === (verified ? '需黑胶 SVIP' : '权益待核对')), 'SVIP rows');
    assert(rows.slice(2, 5).every((row) => row.disabled === !verified && row.caption === (verified ? '' : '权益待核对')), 'verified VIP rows');
    assert(rows.slice(5).every((row) => !row.disabled && !row.caption), 'free rows');
    // Glyph ranges have different ascents for Latin and CJK fallback fonts.
    // Measure the laid-out name and caption boxes, not the individual glyphs.
    lastChoiceLayout = rows.map((row) => {
      const label = row.radio.closest('label');
      return { quality: row.name, top: Math.round(label.getBoundingClientRect().top),
        name: one('.download-quality-name', label).getBoundingClientRect().toJSON(),
        caption: one('small', label)?.getBoundingClientRect().toJSON() ?? null };
    });
    const firstRow = new Map();
    for (const { top, name, caption } of lastChoiceLayout) {
      assert(name.width > 0 && name.height > 0, 'quality name has a visible layout box');
      assert(!caption || (caption.width > 0 && caption.height > 0 &&
        caption.top >= name.bottom && Math.abs(caption.left - name.left) < 1),
        'rights caption stays under the name');
      assert(!firstRow.has(top) || Math.abs(firstRow.get(top) - name.top) < 1,
        'quality names align even when a neighbour has an access caption');
      firstRow.set(top, name.top);
    }
  }
  function retainedVivid() {
    return settings()?.textContent.includes('保留已保存的音质：臻音全景声') &&
      choices().every((row) => !row.checked);
  }
  const readSettled = () => choices().length === valueOrder.length && !dialog();
  const readFeedback = () => one('[data-settings-download-quality-feedback]', settings())?.textContent.trim();
  const refresh = () => button('刷新下载音质', settings());
  let stage = 'startup', count = 0;
  async function capture(label, extra = {}, group = choiceGroup()) {
    stage = label;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    assert(document.documentElement.scrollWidth <= innerWidth, 'no document overflow');
    const rows = choices(group).map(({ radio: _radio, ...row }) => row);
    await emit('ux_' + String(++count).padStart(2, '0') + '_' + label, {
      rows, inline: !!choiceGroup() && !dialog(), qualityReads: structuredClone(qualityReads),
      qualityWrites: structuredClone(qualityWrites), ...extra,
    });
  }
  async function openSettings() {
    click(await wait(() => button('打开设置'), 'header settings control'));
    await wait(() => {
      if (!readSettled()) return false;
      for (let node = choiceGroup(); node; node = node.parentElement) {
        if (node.matches('.page-enter-active,.page-leave-active') || getComputedStyle(node).opacity !== '1')
          return false;
      }
      return true;
    }, 'painted inline default download choices');
    choiceGroup().scrollIntoView({ block: 'center' });
  }
  async function leaveSettings() {
    click(button('返回上一页'));
    await wait(() => !settings() && one('.track-row[data-track-id="1"]'), 'return to synthetic song');
  }
  function observeNativeSettings() {
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const nativeRead = client.downloadQualityPreferences, nativeWrite = client.downloadCommand;
    assert(typeof nativeRead === 'function' && typeof nativeWrite === 'function', 'typed native download bridge');
    client.downloadQualityPreferences = async (owner, refresh) => {
      const observation = { owner, refresh };
      qualityReads.push(observation);
      try {
        const reply = await nativeRead.call(client, owner, refresh);
        observation.result = { epoch: reply.epoch, revision: reply.revision, options: structuredClone(reply.options) };
        return reply;
      } catch (error) {
        observation.failed = true;
        throw error;
      }
    };
    client.downloadCommand = async (owner, command) => {
      if (['default_quality', 'preferences'].includes(command.kind)) {
        const observation = { owner, command: structuredClone(command) };
        qualityWrites.push(observation);
        const reply = await nativeWrite.call(client, owner, command);
        observation.saved = reply.epoch === owner && !reply.failed?.length;
        return reply;
      }
      return nativeWrite.call(client, owner, command);
    };
    return client;
  }
  async function idlePlayback() {
    const player = await invoke('player_snapshot', { epoch });
    const queue = await invoke('player_queue', { epoch });
    assert(player.epoch === epoch && queue.epoch === epoch && player.queueRevision === queue.revision,
      'same account and queue for native playback inspection');
    return {
      preference: player.qualityPreferences.quality,
      transport: { state: player.state, playId: player.playId, entryId: player.entryId,
        position: player.position, queueRevision: queue.revision, current: queue.current,
        entries: queue.tracks.map((track) => track.entryId) },
    };
  }
  async function verifyPlaybackDefault(client, savedDownload) {
    const nativeSave = client.savePlaybackQualityPreference;
    assert(typeof client.playbackQualityPreferences === 'function' && typeof nativeSave === 'function',
      'typed native playback preferences bridge');
    const writes = [];
    client.savePlaybackQualityPreference = async (owner, revision, quality) => {
      const attempt = { owner, revision, quality };
      writes.push(attempt);
      const result = await nativeSave.call(client, owner, revision, quality);
      attempt.result = { epoch: result.epoch, revision: result.revision, quality: result.quality, damaged: result.damaged };
      return result;
    };
    try {
      await wait(() => playbackRadio('standard')?.checked && playbackRadio('exhigh'), 'inline default playback choices');
      const preference = await client.playbackQualityPreferences(epoch, false);
      const before = await idlePlayback();
      assert(preference.epoch === epoch && !preference.damaged && preference.quality === 'standard' &&
        before.preference === 'standard' && before.transport.state === 'idle' && !before.transport.playId,
      'isolated idle player starts at standard');
      click(playbackRadio('exhigh'));
      await wait(() => writes[0]?.result?.quality === 'exhigh' && playbackRadio('exhigh')?.checked &&
        !playbackRadio('exhigh').matches(':disabled,[aria-disabled=true]') &&
        document.activeElement === playbackRadio('exhigh'), 'native playback default saved with focus');
      click(playbackRadio('exhigh'));
      const after = await idlePlayback();
      const readback = await client.playbackQualityPreferences(epoch, false);
      const download = await client.downloadQualityPreferences(epoch, false);
      assert(writes.length === 1 && writes[0].owner === epoch && writes[0].revision === preference.revision &&
        writes[0].quality === 'exhigh' && writes[0].result.epoch === epoch &&
        readback.epoch === epoch && readback.revision === writes[0].result.revision &&
        readback.revision !== preference.revision && readback.quality === 'exhigh' && !readback.damaged,
      'one revision-bound playback default write is durable; reselect did not write again');
      assert(after.preference === 'exhigh' && JSON.stringify(after.transport) === JSON.stringify(before.transport),
        'changing the playback default does not start playback or move the idle queue');
      assert(download.epoch === epoch && download.revision === savedDownload.revision &&
        Object.entries(savedDownload.options).every(([key, value]) => download.options[key] === value) &&
        choices()[4]?.checked && qualityWrites.length === 1,
      'changing the playback default does not touch the separate download setting');
      return { before, after, priorRevision: preference.revision, write: writes[0],
        download: { quality: download.options.quality, revision: download.revision } };
    } finally {
      client.savePlaybackQualityPreference = nativeSave;
    }
  }
  try {
    await wait(() => one('.track-row[data-track-id="1"]'), 'synthetic catalogue track');
    window.__downloadPageAck = null;
    await emit('download-page-fixture', { mode: 'seed_quality' });
    await wait(() => window.__downloadPageAck, 'synthetic startup membership failure and seed');
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);
    ({ epoch } = await invoke('account_snapshot'));
    const client = observeNativeSettings();

    await openSettings();
    assertChoices(false);
    assert(retainedVivid(), 'unverified, hidden stored Vivid survives initial open');
    await capture('startup_unknown', { retained: true });

    click(refresh());
    const message = await wait(() => qualityReads.some((read) => read.refresh && read.failed) &&
      refresh() && !refresh().matches(':disabled,[aria-disabled=true]') && readFeedback(), 'refresh failure and retry');
    assert(retainedVivid() && !qualityWrites.length, 'failed read keeps prior value without a write');
    await capture('manual_failure', { retained: true, message });
    click(refresh());
    const recovered = await wait(() => readSettled() && choices()[2]?.checked && !choices()[4]?.disabled &&
      qualityReads.find((read) => read.refresh && read.result?.options.quality === 'jyeffect')?.result, 'membership recovery');
    assertChoices(true);
    assert(choices()[2].checked && !retainedVivid(), 'verified Vinyl read clamps the saved default before returning');
    await capture('membership_recovered', { clamped: 'jyeffect' });

    click(choices()[2].radio);
    assert(!qualityWrites.length, 'choosing the selected quality is inert');
    click(choices()[4].radio);
    const saved = await wait(() => choices()[4]?.checked && !choices()[4]?.disabled &&
      qualityReads.find((read) => read.refresh === false && read.result?.options.quality === 'lossless')?.result,
    'inline save reads back the new native default');
    const expected = { kind: 'default_quality', revision: recovered.revision, quality: 'lossless' };
    assert(qualityWrites.length === 1 && qualityWrites[0].owner === epoch && qualityWrites[0].saved &&
      Object.keys(qualityWrites[0].command).length === 3 &&
      Object.entries(expected).every(([key, value]) => qualityWrites[0].command[key] === value),
    'one native write changes only the default quality');
    assert(saved.epoch === epoch && saved.revision !== recovered.revision &&
      Object.entries(recovered.options).every(([key, value]) => key === 'quality' || saved.options[key] === value),
    'native readback preserves all other download options');
    await wait(() => document.activeElement === choices()[4]?.radio, 'saved inline choice keeps keyboard focus');
    await leaveSettings();
    await openSettings();
    assertChoices(true);
    assert(choices()[4].checked && !retainedVivid(), 'lossless saved on reopen');
    await capture('default_saved');
    stage = 'playback_default';
    const playback = await verifyPlaybackDefault(client, saved);
    await capture('playback_default_saved', { playback });
    await leaveSettings();

    const more = await wait(() => one('.track-row[data-track-id="1"] [aria-label^="更多操作"]'), 'return to song');
    click(more);
    const download = await wait(
      () => one('.action-menu[aria-label="歌曲操作"] [data-menu-key="download"]'),
      'single song download action',
    );
    click(download);
    const quality = await wait(() => one('select[aria-label="下载音质"]', dialog()), 'single download form');
    assert(quality.value === 'lossless', 'single-download form starts at the saved default');
    quality.value = 'exhigh';
    quality.dispatchEvent(new Event('change', { bubbles: true }));
    assert(quality.value === 'exhigh', 'per-download choice');
    await capture('single_download_choice', { selected: quality.value });
    click(button('开始下载', dialog()));
    const managerOrigin = await wait(() => !dialog() && managerSettingsButton(), 'download task accepted and downloads page entered');

    const task = await wait(async () => {
      const view = await invoke('downloads', { epoch, query: {
        tab: 'completed', category: 'song', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 100,
      } });
      return view.tasks.find((row) => row.track.id === '1' && row.status === 'complete' && !row.cleanupPending);
    }, 'native synthetic download publication');
    assert(task.requestedQuality === 'exhigh' && task.actualQuality === 'standard', 'native request and authoritative actual quality');
    click(managerOrigin);
    const managerChoices = await wait(() => one('.download-quality-choices', dialog()), 'legacy download-manager options');
    assert(choices(managerChoices).find((row) => row.name === '无损')?.checked &&
      one('[aria-label="保存格式"]', dialog()), 'manager still has extra options and shows the unchanged default');
    await capture('default_preserved', { requested: task.requestedQuality, actual: task.actualQuality }, managerChoices);
    click(button('取消', dialog()));
    await wait(() => !dialog() && document.activeElement === managerOrigin, 'download manager settings restores focus');
    await emit('complete', { ok: true, downloadQualityUi: true, stageCount: count,
      realMusicRequests: 0, realAccountWrites: 0, qualityWrites });
  } catch (error) {
    await emit('failed', { step: stage, stageCount: count, message: String(error), stack: error.stack,
      viewport: [innerWidth, innerHeight], lastChoiceLayout, qualityReads, qualityWrites });
  }
})();
