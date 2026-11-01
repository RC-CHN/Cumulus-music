import { computed, onScopeDispose, ref, watch, type Ref } from 'vue';
import { failure } from '../services/contracts.ts';
import type { MiniClient, MiniView } from '../services/mini.ts';

/** The saved policy is an intent, never proof of the native window flag. */
export function useMiniPin(
  view: Ref<MiniView | null>,
  client: Pick<MiniClient, 'preferences' | 'applyPreferences'>,
  refresh: () => Promise<MiniView | undefined>,
  busy: Ref<boolean>,
) {
  const issue = ref(''),
    readIssue = ref(''),
    needsRead = ref(false);
  let alive = true;
  const current = (epoch: string) => alive && view.value?.media.epoch === epoch;
  watch(
    () => view.value?.media.epoch,
    () => {
      issue.value = readIssue.value = '';
      needsRead.value = false;
    },
  );
  onScopeDispose(() => {
    alive = false;
  });
  const unread = computed(() => needsRead.value || view.value?.pinApplied == null);
  const unapplied = computed(
    () =>
      !!view.value &&
      !view.value.preferences.damaged &&
      !unread.value &&
      view.value.pinApplied !== view.value.preferences.alwaysOnTop,
  );
  const label = computed(() =>
    unread.value
      ? '重新读取置顶状态'
      : view.value?.preferences.damaged
        ? '修复置顶设置'
        : unapplied.value
          ? '重试应用置顶设置'
          : '始终置顶',
  );
  const notice = computed(() => {
    if (!view.value) return '';
    const state = view.value.preferences.damaged
      ? '置顶设置已损坏，可明确修复。'
      : unread.value
        ? '无法确认窗口置顶状态，请重新读取。'
        : unapplied.value
          ? '置顶设置已保存，但尚未应用到窗口，请重试。'
          : '';
    return [...new Set([issue.value, readIssue.value, state].filter(Boolean))].join(' ');
  });
  async function apply() {
    const shown = view.value;
    if (!shown || busy.value) return;
    busy.value = true;
    issue.value = readIssue.value = '';
    try {
      // A failed observation must not turn a retry into another toggle.
      if (!unread.value) {
        if (shown.preferences.damaged) {
          await client.preferences(shown.ticket, { kind: 'repair' });
        } else if (unapplied.value) {
          await client.applyPreferences(shown.ticket, shown.preferences.revision!);
        } else {
          await client.preferences(shown.ticket, {
            kind: 'change',
            expected_revision: shown.preferences.revision!,
            always_on_top: !shown.preferences.alwaysOnTop,
          });
        }
      }
    } catch (error) {
      if (current(shown.media.epoch)) issue.value = failure(error).message;
    }
    try {
      const observed = await refresh();
      if (current(shown.media.epoch)) {
        if (!observed || observed.media.epoch !== shown.media.epoch)
          throw { code: 'stale', message: '置顶状态尚未读到，请重新读取', retryable: true };
        needsRead.value = false;
      }
    } catch (error) {
      if (current(shown.media.epoch)) {
        needsRead.value = true;
        readIssue.value = failure(error).message;
      }
    } finally {
      busy.value = false;
    }
  }
  return { label, notice, apply, selected: computed(() => view.value?.pinApplied ?? undefined) };
}
