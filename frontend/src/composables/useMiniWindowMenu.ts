import { onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  MiniClient,
  MiniView,
  MiniLyricMenuView,
  MiniLyricMenuAction,
} from '../services/mini.ts';

export function useMiniWindowMenu(
  view: Ref<MiniView | null>,
  client: Pick<MiniClient, 'lyricMenu' | 'lyricMenuAction'>,
  busy: Ref<boolean>,
  refresh: () => Promise<MiniView | undefined>,
) {
  const lyrics = shallowRef<MiniLyricMenuView>(),
    loading = ref(false),
    working = ref(false),
    readIssue = ref(''),
    issue = ref('');
  let alive = true,
    serial = 0,
    opened = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const same = (epoch: string) => alive && view.value?.media.epoch === epoch;
  function closed() {
    opened = false;
    clearTimeout(timer);
    serial++;
    loading.value = false;
  }
  watch(
    () => view.value?.media.epoch,
    () => {
      closed();
      lyrics.value = undefined;
      readIssue.value = issue.value = '';
    },
  );
  onScopeDispose(() => {
    alive = false;
    closed();
  });
  function schedule() {
    clearTimeout(timer);
    if (alive && opened) timer = setTimeout(() => void load(false), 750);
  }
  async function load(initial: boolean) {
    const shown = view.value;
    if (!alive || !opened || !shown) return;
    const request = ++serial;
    if (initial) {
      loading.value = true;
      lyrics.value = undefined;
      readIssue.value = '';
    }
    if (busy.value || working.value) {
      schedule();
      return;
    }
    try {
      const value = await client.lyricMenu(shown.ticket);
      if (same(shown.media.epoch) && opened && request === serial) {
        lyrics.value = value;
        readIssue.value = '';
        if (initial && !value.error) issue.value = '';
      }
    } catch (error) {
      if (same(shown.media.epoch) && opened && request === serial) {
        lyrics.value = undefined;
        readIssue.value = failure(error).message;
      }
    } finally {
      if (alive && opened && request === serial) {
        loading.value = false;
        schedule();
      }
    }
  }
  function read() {
    if (!alive) return;
    closed();
    opened = true;
    return load(true);
  }
  async function action(command: MiniLyricMenuAction) {
    const shown = view.value,
      context = lyrics.value;
    if (!alive || !shown || !context || busy.value || working.value) return;
    working.value = true;
    issue.value = '';
    lyrics.value = undefined;
    try {
      await client.lyricMenuAction(context.ticket, command);
    } catch (error) {
      if (same(shown.media.epoch)) issue.value = failure(error).message;
    } finally {
      if (same(shown.media.epoch)) await refresh().catch(() => {});
      working.value = false;
    }
  }
  return { lyrics, loading, working, readIssue, issue, read, closed, action };
}
