import { ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type {
  VerticalZoneClient,
  VerticalZoneContent,
  VerticalZoneLocation,
  VerticalZoneMenu,
  VerticalZoneTab,
} from '../services/verticalZone';

export interface ZoneViewport {
  top: number;
  focus: string;
  rails: Map<string, number>;
}

export function useVerticalZone(
  core: VerticalZoneClient,
  account: () => AccountView,
  active: () => boolean,
  route: () => VerticalZoneLocation | undefined,
  expired: () => void,
) {
  const menu = shallowRef<VerticalZoneMenu>();
  const data = shallowRef<VerticalZoneContent>();
  const tabs = shallowRef<VerticalZoneTab[]>([]);
  const loading = ref(false);
  const error = ref('');
  const viewports = new Map<string, ZoneViewport>();
  // In-memory navigation has no URL to rewrite. Canonicalize the specific
  // history entry so Back uses the server's real key without matching names again.
  let canonicalKeys = new WeakMap<VerticalZoneLocation, string>();
  let owner: string | undefined;
  let menuSerial = 0,
    pageSerial = 0;

  function viewport(location: VerticalZoneLocation) {
    const key = JSON.stringify([
      location.pageCode,
      location.tabKey || (location.tabKeyName ? ['name', location.tabKeyName] : 'default'),
    ]);
    const current = viewports.get(key) ?? { top: 0, focus: '', rails: new Map() };
    viewports.delete(key);
    viewports.set(key, current);
    if (viewports.size > 16) viewports.delete(viewports.keys().next().value!);
    return current;
  }

  function problem(cause: unknown) {
    const issue = failure(cause);
    if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
    return issue.message;
  }

  async function loadMenu() {
    if (!owner || !active()) return;
    const epoch = owner,
      serial = ++menuSerial;
    try {
      const response = await core.verticalZoneMenu(epoch);
      if (serial === menuSerial && owner === epoch && response.epoch === epoch)
        menu.value = response.value;
    } catch (cause) {
      // The original choice bar preserves the last usable menu on failure.
      if (serial === menuSerial && owner === epoch) problem(cause);
    }
  }

  async function read() {
    const location = route(),
      epoch = owner;
    if (!location || !epoch) return;
    const serial = ++pageSerial;
    const current = () => owner === epoch && serial === pageSerial && route() === location;
    loading.value = true;
    error.value = '';
    try {
      const effectiveKey = canonicalKeys.get(location) ?? location.tabKey;
      const response = await core.verticalZonePage(
        epoch,
        location.pageCode,
        effectiveKey,
        location.from === 'searchHotExplain' && !effectiveKey ? location.tabKeyName : undefined,
      );
      if (current() && response.epoch === epoch) {
        if (response.value.layout === 'tabs' && response.value.activeTab)
          canonicalKeys.set(location, response.value.activeTab.key);
        else if (effectiveKey) canonicalKeys.set(location, 'default');
        data.value = response.value;
        if (response.value.layout !== 'fallback' || response.value.tabs.length)
          tabs.value = response.value.tabs;
      }
    } catch (cause) {
      if (current()) error.value = problem(cause);
    } finally {
      if (current()) loading.value = false;
    }
  }

  watch(
    [() => account().epoch, () => account().status, active],
    ([epoch, status, visible]) => {
      const next = status === 'authenticated' ? epoch : undefined;
      if (owner !== next) {
        owner = next;
        menuSerial++;
        pageSerial++;
        menu.value = undefined;
        data.value = undefined;
        tabs.value = [];
        loading.value = false;
        error.value = '';
        viewports.clear();
        canonicalKeys = new WeakMap();
      }
      if (visible) void loadMenu();
    },
    { flush: 'sync', immediate: true },
  );
  watch(
    [route, () => account().epoch, () => account().status],
    ([next], previous) => {
      if (next?.pageCode !== previous[0]?.pageCode) tabs.value = [];
      pageSerial++;
      data.value = undefined;
      error.value = '';
      loading.value = false;
      void read();
    },
    { flush: 'sync', immediate: true },
  );
  return { menu, data, tabs, loading, error, read, viewport };
}

export type VerticalZoneModel = ReturnType<typeof useVerticalZone>;
