// Real WebView controls, with synthetic CoreClient receipts instead of effects service requests.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const wait = async (condition, message) => {
    for (let index = 0; index < 150; index++) {
      const value = condition();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  };
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const button = (root, name) => [...(root?.querySelectorAll('button') ?? [])].find(
    (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
  );
  const enabled = (node) => node && !node.disabled && node.getAttribute('aria-disabled') !== 'true' && node;
  const click = (node) => {
    assert(enabled(node), 'enabled control expected');
    node.focus({ preventScroll: true });
    node.click();
  };
  const controls = () => one('.effect-parametric');
  const action = (name) => controls()?.querySelector(`button[data-peq-action="${name}"]`);
  const phase = (name) => controls()?.getAttribute('data-peq-phase') === name;

  const effect = { id: 731, name: '合成参数均衡音效', subtitle: '合成说明' };
  const makeView = (state, overrides = {}) => ({
    id: effect.id, phase: state, attempt: null, release: null, registration: null, canStop: false,
    stopNeedsRetry: false, activeId: null, issue: null, ...overrides,
  });
  const prepared = makeView('prepared', { attempt: 'synthetic-attempt-A' });
  const configured = makeView('configured', {
    release: 'synthetic-release-A', registration: 'registered', canStop: true, activeId: effect.id,
  });
  const deferred = { ...configured, phase: 'deferred' };
  const uncertain = makeView('recovery_required', {
    release: 'synthetic-release-B', registration: 'stop_uncertain', canStop: true, stopNeedsRetry: true,
    issue: '停止结果待核对',
  });
  const observed = { ...uncertain, release: 'synthetic-release-C', issue: '停止结果仍待核对' };
  const released = makeView('idle', { registration: 'released' });

  const reads = [], mutations = [];
  let current = makeView('idle'), stage = 'open';
  try {
    let account;
    for (let index = 0; index < 100; index++) {
      account = await invoke('account_snapshot');
      if (account.status === 'authenticated') break;
      await sleep(65);
    }
    assert(account?.status === 'authenticated', 'isolated synthetic account did not restore');
    const provides = one('#app')?.__vue_app__?._context.provides;
    assert(provides, 'production Vue app missing');
    const core = provides[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    assert(core, 'CoreClient injection missing');
    const scope = (epoch, id) => assert(epoch === account.epoch && id === effect.id, 'wrong account/effect scope');
    core.effectCatalogue = async (epoch, category) => {
      assert(epoch === account.epoch && category === 'highly_recommended', 'wrong catalogue scope');
      return [effect];
    };
    core.effectDetail = async (epoch, id) => {
      scope(epoch, id);
      return { ...effect, access: 'allowed' };
    };
    core.effectPeqStatus = async (epoch, id) => {
      scope(epoch, id);
      reads.push(['status', current.phase]);
      if (current === deferred && reads.filter(([, phase]) => phase === 'deferred').length === 2)
        current = configured;
      return { ...current };
    };
    core.effectPeqReconcile = async (epoch, id) => {
      scope(epoch, id);
      reads.push(['reconcile', current.phase]);
      if (current === uncertain) current = observed;
      return { ...current };
    };
    core.effectPeqPrepare = async (epoch, id) => {
      scope(epoch, id);
      assert(current.phase === 'idle', 'prepare must be explicit and only once');
      mutations.push('prepare');
      current = prepared;
      return { ...current };
    };
    core.effectPeqActivate = async (epoch, id, attempt) => {
      scope(epoch, id);
      assert(current === prepared && attempt === prepared.attempt, 'activation must consume the exact prepared attempt');
      mutations.push('activate');
      current = deferred;
      return { ...current };
    };
    core.effectPeqStop = async (epoch, id, release, retryUnknown) => {
      scope(epoch, id);
      if (current === configured) {
        assert(release === configured.release && retryUnknown === false, 'initial stop must use its exact release ticket');
        mutations.push('stop');
        current = uncertain;
      } else {
        assert(current === observed && release === observed.release && retryUnknown === true, 'confirmed retry must use the latest exact ticket');
        mutations.push('retry-stop');
        current = released;
      }
      return { ...current };
    };

    click(await wait(() => enabled(one('.equalizer-toggle')), 'EQ launcher'));
    const eq = await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog');
    click(button(eq, '音效目录'));
    const catalogue = await wait(() => one('.effect-directory[open]'), 'effects directory');
    click(await wait(() => enabled(catalogue.querySelector('.effect-directory-card')), 'synthetic catalogue item'));
    await wait(() => controls() && phase('idle') && reads.some(([kind]) => kind === 'status'), 'standalone controls read initial status');
    stage = 'status';
    const readCount = reads.length;
    click(await wait(() => enabled(action('status')), 'status action'));
    await sleep(0);
    await wait(() => reads.length > readCount && phase('idle') && enabled(action('status')), 'status read completed');
    assert(mutations.length === 0, 'reading status must not prepare, activate, or stop');
    assert(!enabled(action('activate')), 'activation requires an explicit prepared attempt');

    stage = 'prepare';
    click(await wait(() => enabled(action('prepare')), 'prepare action'));
    await wait(() => phase('prepared') && enabled(action('activate')), 'prepared component and explicit activate');
    assert(mutations.join(',') === 'prepare', 'preparation must not start registration');

    stage = 'activate';
    click(action('activate'));
    await wait(() => phase('deferred') && enabled(action('stop')), 'deferred application is not yet configured');
    await wait(() => phase('configured') && enabled(action('stop')), 'configured observed without a manual action');
    assert(reads.filter(([kind, state]) => kind === 'status' && state === 'deferred').length === 2,
      'deferred application advances by read-only status');
    assert(mutations.join(',') === 'prepare,activate', 'observing deferred state makes no writes');

    stage = 'stop';
    click(action('stop'));
    await wait(() => phase('recovery_required') && enabled(action('stop')), 'uncertain stop recovery');
    assert(mutations.join(',') === 'prepare,activate,stop', 'unknown stop must not retry automatically');

    stage = 'reconcile';
    const recoveryReadCount = reads.length;
    click(await wait(() => enabled(action('status')), 'reconcile action'));
    await wait(() => reads.length > recoveryReadCount && phase('recovery_required') && controls()?.textContent.includes(observed.issue) && enabled(action('stop')), 'status read while stop outcome is unknown');
    assert(current === observed && mutations.length === 3 && reads.some(([kind, state]) => kind === 'reconcile' && state === 'recovery_required'), 'reading unknown stop only refreshes its ticket');

    stage = 'confirm';
    click(action('stop'));
    await wait(() => enabled(action('confirm-stop')) && enabled(action('cancel-stop')), 'explicit retry confirmation');
    assert(mutations.length === 3, 'opening confirmation must not submit stop');
    click(action('cancel-stop'));
    await wait(() => !action('confirm-stop') && enabled(action('stop')), 'cancel retry confirmation');
    assert(mutations.length === 3, 'cancel must not submit stop');
    click(action('stop'));
    click(await wait(() => enabled(action('confirm-stop')), 'retry confirmation reopened'));
    await wait(() => phase('idle') && !enabled(action('stop')), 'confirmed release terminal state');
    assert(mutations.join(',') === 'prepare,activate,stop,retry-stop', 'exact operation sequence');
    assert(!/synthetic-(attempt|release)/.test(catalogue.innerHTML), 'opaque tickets never render into the directory');
    await emit('complete', {
      ok: true, effectPeqControlVerified: true, injectedCoreClient: true, mutationOrder: mutations,
      deferredStatusObserved: reads.filter(([kind, state]) => kind === 'status' && state === 'deferred').length === 2,
      readWhileStopUncertain: reads.some(([, state]) => state === 'recovery_required'),
    });
  } catch (error) {
    await emit('failed', { step: stage, message: String(error), phase: controls()?.getAttribute('data-peq-phase'), mutations, reads });
  }
})();
