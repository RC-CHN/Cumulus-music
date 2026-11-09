// Real WebView download-row create form and two separate native write receipts.
(async () => {
  const one = (s, root = document) => root?.querySelector(s);
  const picker = () => one('dialog[data-download-collect][open]:not(.modal-leave-active)');
  const menu = () => one('.action-menu[aria-label="下载资源操作"]');
  const form = () => one('[data-download-create-phase]', picker());
  const phase = () => form()?.dataset.downloadCreatePhase;
  const button = (text, root = document) => [...(root?.querySelectorAll('button') ?? [])]
    .find((item) => item.getAttribute('aria-label') === text || item.textContent.trim() === text);
  const assert = (ok, why) => { if (!ok) throw Error(why); };
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, details = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: {stage, ...details},
  });
  let last = 'startup', count = 0, originalQueue, epoch, restore;
  const commands = [];
  async function wait(check, label, attempts = 260) {
    last = label;
    for (let i = 0; i < attempts; i++) {
      const result = await check();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  async function control(stage, ack, mode) {
    window[ack] = null;
    await emit(stage, {mode});
    const result = await wait(() => window[ack], 'fixture ' + mode, 1200);
    if ('ok' in result) assert(result.ok, result.message);
  }
  const server = (mode) => control('collect-fixture', '__collectFixtureAck', mode);
  const seed = (mode) => control('download-page-fixture', '__downloadPageAck', mode);
  const row = (id) => one('[data-download-task="' + CSS.escape(id) + '"]');
  const more = (id) => one('[aria-label^="管理下载"]', row(id));
  const click = (node) => {
    assert(node && !node.disabled && node.getAttribute('aria-disabled') !== 'true', 'enabled control');
    node.focus({preventScroll:true});
    node.click();
  };
  const create = () => button('创建', picker());
  const check = () => button('核对结果', picker());
  const field = () => one('input[aria-label="歌单名称"]', picker());
  const privacy = () => one('input[aria-label="设为隐私歌单"]', picker());
  async function open(id) {
    const current = await wait(() => row(id), 'download source row');
    current.scrollIntoView({block:'nearest'});
    click(more(id));
    await wait(() => menu()?.style.visibility === 'visible' && menu().contains(document.activeElement),
      'focused download menu');
    click(button('收藏', menu()));
    await wait(() => picker()?.querySelector('[data-playlist-id="2"]') &&
      button('创建新歌单', picker()), 'native scoped picker with a create target');
    click(button('创建新歌单', picker()));
    await wait(() => phase() === 'prepared' && field() && document.activeElement === field(),
      'create confirmation focuses the name');
    assert(one('h2', picker()).textContent.trim() === '创建歌单' && !privacy().checked &&
      create().disabled, 'official create title, empty disabled form and public default');
  }
  async function submit(name, privatePlaylist) {
    field().value = name;
    field().dispatchEvent(new Event('input', {bubbles:true}));
    await wait(() => !create()?.disabled && create(), 'valid playlist name enables creation');
    if (privatePlaylist) click(privacy());
    click(create());
  }
  async function capture(label) {
    await wait(() => !one('.modal-enter-active,.modal-leave-active,.collect-step-enter-active,.collect-step-leave-active'),
      'settled collection dialog');
    const queue = await invoke('player_queue', {epoch});
    const {revision, current, tracks} = queue;
    assert(JSON.stringify({revision, current, tracks}) === originalQueue, 'collection preserves native queue');
    const stage = 'ux_' + String(++count).padStart(2,'0') + '_' + label;
    window.__downloadCollectionRecorded = '';
    await emit(stage, {
      actual: document.visibilityState === 'visible',
      phase: phase() ?? null,
      notice: one('[data-download-create-notice]', picker())?.textContent.trim() ?? null,
      focused: document.activeElement === field() ? 'name' : document.activeElement === check() ? 'reconcile' : null,
      createButton: !!create(),
      commands,
    });
    await wait(() => window.__downloadCollectionRecorded === stage, 'native capture ' + label);
  }
  try {
    await wait(() => one('.track-row'), 'initial daily page');
    await seed('seed');
    await seed('cloud_comment_targets');
    click(button('下载管理'));
    await wait(() => one('.downloads-page') && !one('#download-content[aria-busy="true"]'), 'download page');
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find(
      (key) => typeof key === 'symbol' && key.description === 'core-client',
    )];
    ({epoch} = await client.accountSnapshot());
    const queue = await invoke('player_queue', {epoch});
    const {revision, current, tracks} = queue;
    originalQueue = JSON.stringify({revision, current, tracks});
    const inventory = await client.downloads(epoch, {
      tab:'completed', category:'song', keyword:'', sort:'none', descending:false, offset:0, limit:20,
    });
    const [ordinary, matched] = ['1','5'].map((id) => inventory.tasks.find((task) => task.track.id === id));
    assert(ordinary.collectionAvailable && matched.collectionAvailable && matched.commentTrackId === '9010',
      'ordinary and public matched-cloud task authority');
    const native = client.downloadCollectCreation;
    client.downloadCollectCreation = async (e, receipt, command) => {
      assert(e === epoch && typeof receipt === 'string' && receipt.length === 32, 'opaque task receipt and account');
      const entry = {receipt, command};
      commands.push(entry);
      const reply = await native(e, receipt, command);
      assert(reply.epoch === e && reply.value.receipt === receipt, 'same native creation receipt');
      entry.outcome = {
        create: reply.value.create.phase, add: reply.value.add?.phase ?? null,
        canAdd: reply.value.canAdd, playlistId: reply.value.create.playlistId,
      };
      return reply;
    };
    restore = () => { client.downloadCollectCreation = native; };

    await open(ordinary.id);
    await capture('form_prepared');
    click(button('关闭歌单操作', picker()));
    await wait(() => !picker() && document.activeElement === more(ordinary.id),
      'cancel restores the original row with no request');
    assert(commands.length === 0, 'open/cancel performs no create command');
    await capture('canceled');

    await open(ordinary.id);
    await submit('合成下载公开歌单', false);
    await wait(() => !picker() && commands.at(-1)?.outcome?.add === 'confirmed' &&
      one('.downloads-page')?.textContent.includes('已收藏到新歌单。'), 'create then add confirms in place');
    await capture('confirmed');

    await server('lost_create');
    await open(ordinary.id);
    await submit('合成下载丢失创建', true);
    await wait(() => phase() === 'pending' && document.activeElement === check(), 'lost create needs explicit readback');
    assert(!create() && commands.at(-1).outcome.add === null, 'lost create never offers another write');
    await capture('create_pending');
    await server('success');
    click(check());
    await wait(() => phase() === 'failed' && one('[data-download-create-notice]', picker())?.textContent.includes('缺少创建回执'),
      'list confirmation cannot prove permission to add');
    assert(!create() && !button('继续收藏', picker()), 'no inferred add or create retry');
    await capture('create_readback_only');
    click(button('关闭歌单操作', picker()));
    await wait(() => !picker(), 'close checked create');

    await server('uncertain');
    await open(matched.id);
    await submit('合成下载丢失收藏', true);
    await wait(() => phase() === 'pending' && commands.at(-1)?.outcome?.add === 'uncertain' &&
      document.activeElement === check(), 'lost add keeps exact receipt and focuses readback');
    await capture('add_pending');
    await server('success');
    click(check());
    await wait(() => !picker() && commands.at(-1)?.outcome?.add === 'confirmed', 'read-only add reconciliation');
    await capture('add_reconciled');
    assert(commands.map(({command}) => command.kind).join(',') === 'create,create,status,create,status',
      'three explicit creates, two explicit readbacks, no replay');
    await emit('complete', {ok:true, stageCount:count, realMusicRequests:0, realAccountWrites:0, commands});
  } catch (error) {
    await emit('failed', {ok:false, stageCount:count, last, error:String(error), commands});
  } finally {
    restore?.();
  }
})();
