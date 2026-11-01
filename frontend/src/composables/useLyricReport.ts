import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type { LyricReport, LyricReportClient } from '../services/lyricReports.ts';

export function useLyricReport(
  epoch: () => string,
  client: LyricReportClient,
  notify: (message: string) => void,
) {
  const open = ref(false),
    reading = ref(false),
    working = ref(false),
    issue = ref(''),
    typeIssue = ref('');
  const record = shallowRef<LyricReport>(),
    selected = ref(''),
    content = ref(''),
    loaded = ref(false),
    recovery = ref(false);
  const ownerEpoch = ref(epoch());
  let alive = true,
    serial = 0,
    source: { epoch: string; kind: 'play' | 'id'; id: string } | undefined;
  const uncertain = computed(
    () =>
      recovery.value || record.value?.phase === 'sending' || record.value?.phase === 'uncertain',
  );
  const canSubmit = computed(
    () =>
      open.value &&
      !reading.value &&
      !working.value &&
      !uncertain.value &&
      loaded.value &&
      !!record.value &&
      ['draft', 'rejected'].includes(record.value.phase) &&
      record.value.options.some((o) => o.key === selected.value) &&
      content.value.length >= 5 &&
      content.value.length <= 50,
  );
  const current = (request: number, owner: string) =>
    alive && serial === request && epoch() === owner;
  function close(force = false) {
    if (working.value && !force) return;
    serial++;
    open.value = false;
    reading.value = working.value = false;
    record.value = undefined;
    source = undefined;
    selected.value = content.value = issue.value = typeIssue.value = '';
    loaded.value = recovery.value = false;
  }
  watch(epoch, () => close(true));
  onScopeDispose(() => {
    close(true);
    alive = false;
  });
  function apply(value: LyricReport, form = true) {
    record.value = value;
    recovery.value = false;
    if (value.phase === 'abandoned') issue.value = '此记录已结束，请关闭后重新填写';
    if (form) {
      selected.value = value.selected ?? '';
      content.value = value.content;
    } else if (selected.value && !value.options.some((o) => o.key === selected.value))
      selected.value = '';
  }
  async function types() {
    const shown = record.value;
    if (!alive || !open.value || !shown || reading.value || working.value || uncertain.value)
      return;
    const request = ++serial;
    reading.value = true;
    loaded.value = false;
    typeIssue.value = '';
    try {
      // Another window can have changed this record while the menu was open.
      // Refresh its revision before the read, without replaying any submission.
      const latest = await client.read(shown.epoch, shown.id);
      if (!current(request, shown.epoch) || !open.value) return;
      if (!['draft', 'rejected'].includes(latest.phase)) {
        apply(latest);
        if (latest.phase === 'sent') {
          notify('反馈信息已发送！');
          reading.value = false;
          close();
        }
        return;
      }
      apply(latest, false);
      const value = await client.types(shown.epoch, shown.id, latest.revision);
      if (current(request, shown.epoch) && open.value) {
        apply(value, false);
        loaded.value = true;
      }
    } catch (error) {
      if (current(request, shown.epoch)) typeIssue.value = failure(error).message;
    } finally {
      if (current(request, shown.epoch)) reading.value = false;
    }
  }
  async function begin(kind: 'play' | 'id', id: string) {
    if (!alive || working.value) return;
    close();
    ownerEpoch.value = epoch();
    source = { epoch: epoch(), kind, id };
    open.value = true;
    reading.value = true;
    const owner = epoch(),
      request = ++serial;
    try {
      const value = await (kind === 'play' ? client.open(owner, id) : client.read(owner, id));
      if (current(request, owner) && open.value) {
        apply(value, !['draft', 'rejected'].includes(value.phase) || !content.value);
        reading.value = false;
        if (value.phase === 'sent') {
          notify('反馈信息已发送！');
          close();
        } else if (['draft', 'rejected'].includes(value.phase)) await types();
      }
    } catch (error) {
      if (current(request, owner)) issue.value = failure(error).message;
    } finally {
      if (current(request, owner)) reading.value = false;
    }
  }
  async function refresh() {
    const shown = record.value;
    if (!shown) {
      if (source) return begin(source.kind, source.id);
      return;
    }
    if (!alive || !open.value || working.value || reading.value) return;
    const request = ++serial;
    reading.value = true;
    issue.value = '';
    try {
      const value = await client.read(shown.epoch, shown.id);
      if (current(request, shown.epoch) && open.value) {
        apply(value, !['draft', 'rejected'].includes(value.phase) || !content.value);
        loaded.value = value.options.length > 0;
        if (value.phase === 'sent') {
          notify('反馈信息已发送！');
          reading.value = false;
          close();
        }
      }
    } catch (error) {
      if (current(request, shown.epoch)) issue.value = failure(error).message;
    } finally {
      if (current(request, shown.epoch)) reading.value = false;
    }
  }
  async function submit() {
    const shown = record.value;
    if (!canSubmit.value || !shown) return;
    const request = ++serial;
    working.value = true;
    issue.value = '';
    const key = selected.value,
      text = content.value;
    try {
      const value = await client.submit(shown.epoch, shown.id, shown.revision, key, text);
      if (current(request, shown.epoch) && open.value) {
        apply(value);
        loaded.value = true;
        if (value.phase === 'sent') {
          notify('反馈信息已发送！');
          working.value = false;
          close();
        }
      }
    } catch (error) {
      if (current(request, shown.epoch) && open.value) {
        recovery.value = true;
        issue.value = failure(error).message;
        // One read only: a lost IPC acknowledgement must never replay POST.
        try {
          const value = await client.read(shown.epoch, shown.id);
          if (current(request, shown.epoch) && open.value) {
            apply(value, !['draft', 'rejected'].includes(value.phase));
            loaded.value = value.options.length > 0;
            if (value.phase === 'sent') {
              notify('反馈信息已发送！');
              working.value = false;
              close();
            }
          }
        } catch {
          /* Preserve the submitted text and block another send until readback. */
        }
      }
    } finally {
      if (current(request, shown.epoch)) working.value = false;
    }
  }
  async function abandon() {
    const shown = record.value;
    if (!alive || !open.value || !shown || working.value || reading.value || !uncertain.value)
      return;
    const request = ++serial;
    working.value = true;
    issue.value = '';
    try {
      await client.abandon(shown.epoch, shown.id, shown.revision);
      if (current(request, shown.epoch)) {
        notify('已停止本地跟踪，服务端反馈不会被撤回');
        working.value = false;
        close();
      }
    } catch (error) {
      if (current(request, shown.epoch)) issue.value = failure(error).message;
    } finally {
      if (current(request, shown.epoch)) working.value = false;
    }
  }
  return {
    ownerEpoch,
    open,
    reading,
    working,
    issue,
    typeIssue,
    record,
    selected,
    content,
    loaded,
    uncertain,
    canSubmit,
    close,
    types,
    refresh,
    submit,
    abandon,
    show: (playId: string) => begin('play', playId),
    showId: (id: string) => begin('id', id),
  };
}
