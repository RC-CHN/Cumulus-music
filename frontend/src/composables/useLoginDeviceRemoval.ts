import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import {
  removalPending,
  removalSms,
  validDeviceRemovalCode,
  type LoginDeviceRemovalClient,
  type LoginDeviceRemovalCommand,
  type LoginDeviceRemovalView,
} from '../services/loginDeviceRemoval.ts';

// A transport failure can occur before native records the intent. Retain only
// the opaque device ID for this epoch, never the SMS code or device details.
let unacknowledgedBegin: { epoch: string; id: string } | undefined;

export function useLoginDeviceRemoval(
  client: LoginDeviceRemovalClient,
  owner: () => string | null,
  absent: () => void,
  expired: () => void,
  now: () => number = () => performance.now(),
) {
  const view = shallowRef<LoginDeviceRemovalView>();
  const busy = ref(false);
  const known = ref(false);
  const readRequired = ref(false);
  const error = ref('');
  const seconds = ref(0);
  const unseen = ref<string>();
  const pending = computed(() => removalPending(view.value?.phase) || !!unseen.value);
  const canBegin = computed(
    () => known.value && !busy.value && !pending.value && !readRequired.value,
  );
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let serial = 0;

  function cooldown(secondsValue: number) {
    if (timer !== undefined) clearTimeout(timer);
    const deadline = now() + Math.max(0, Math.min(60, Number(secondsValue) || 0)) * 1000;
    const tick = () => {
      seconds.value = Math.max(0, Math.ceil((deadline - now()) / 1000));
      timer = seconds.value > 0 ? setTimeout(tick, 1000) : undefined;
    };
    tick();
  }
  function clear() {
    serial++;
    cooldown(0);
    view.value = undefined;
    error.value = '';
    known.value = false;
    busy.value = false;
    readRequired.value = false;
    const epoch = owner();
    if (unacknowledgedBegin?.epoch !== epoch) unacknowledgedBegin = undefined;
    unseen.value = unacknowledgedBegin?.id;
  }
  function accept(next: LoginDeviceRemovalView | null, notify = false) {
    const previous = view.value;
    view.value = next ?? undefined;
    cooldown(next?.resendAfterSeconds ?? 0);
    if (unseen.value && next?.id === unseen.value) {
      if (unacknowledgedBegin?.id === unseen.value) unacknowledgedBegin = undefined;
      unseen.value = undefined;
    }
    known.value = true;
    readRequired.value = !!unseen.value;
    if (
      notify &&
      next?.phase === 'observed_absent' &&
      next.observedPresent === false &&
      (previous?.id !== next.id || previous.phase !== 'observed_absent')
    )
      absent();
  }
  function fail(cause: unknown, message: string) {
    const code = failure(cause).code;
    if (code === 'session_expired' || code === 'profile_changed') {
      clear();
      expired();
    } else error.value = message;
  }
  function run(work: (epoch: string, active: () => boolean) => Promise<void>) {
    const epoch = owner();
    if (!epoch || disposed || busy.value) return;
    const request = ++serial;
    const active = () => !disposed && request === serial && epoch === owner();
    busy.value = true;
    error.value = '';
    return work(epoch, active).finally(() => {
      if (active()) busy.value = false;
    });
  }
  function read(notify = true) {
    return run(async (epoch, active) => {
      try {
        const response = await client.accountLoginDeviceRemoval(epoch);
        if (!active()) return;
        if (response.epoch !== epoch) throw Error('owner mismatch');
        accept(response.value, notify);
        if (unseen.value) error.value = '尚未取得移除回执，请稍后重新读取。';
      } catch (cause) {
        if (!active()) return;
        readRequired.value = true;
        fail(cause, '暂时无法读取移除进度，请重试。');
      }
    });
  }
  function command(command: LoginDeviceRemovalCommand) {
    const current = view.value;
    if (command.kind === 'begin') {
      if (!canBegin.value) return;
    } else {
      if (readRequired.value || !current || current.id !== command.operation_id) return;
      if (command.kind === 'send_sms' && (!removalSms(current.phase) || seconds.value > 0)) return;
      if (
        command.kind === 'submit_sms' &&
        ((current.phase !== 'sms_ready' && current.phase !== 'sms_send_uncertain') ||
          !validDeviceRemovalCode(command.code))
      )
        return;
      if (command.kind === 'cancel' && !removalSms(current.phase)) return;
      if (command.kind === 'reconcile' && current.phase !== 'recovery_required') return;
    }
    return run(async (epoch, active) => {
      if (command.kind === 'begin') {
        unseen.value = command.device_id;
        unacknowledgedBegin = { epoch, id: command.device_id };
      }
      try {
        const response = await client.accountLoginDeviceRemovalCommand(epoch, command);
        if (!active()) return;
        const id = command.kind === 'begin' ? command.device_id : command.operation_id;
        if (response.epoch !== epoch || response.value.id !== id) throw Error('operation mismatch');
        accept(response.value, true);
      } catch (cause) {
        if (!active()) return;
        // These native errors are returned before an intent could send kickoff.
        const code = failure(cause).code;
        if (
          command.kind === 'begin' &&
          ['stale', 'invalid_input', 'write_uncertain', 'busy', 'request_not_sent'].includes(code)
        ) {
          unseen.value = undefined;
          if (unacknowledgedBegin?.id === command.device_id) unacknowledgedBegin = undefined;
        }
        readRequired.value = true;
        fail(cause, '暂未确认操作结果，请先重新读取移除进度。');
      }
    });
  }
  const stop = watch(
    owner,
    () => {
      clear();
      void read(false);
    },
    { immediate: true, flush: 'sync' },
  );
  onScopeDispose(() => {
    disposed = true;
    stop();
    if (timer !== undefined) clearTimeout(timer);
    serial++;
    view.value = undefined;
  });
  return {
    view,
    busy,
    known,
    pending,
    error,
    seconds,
    unseen,
    canBegin,
    readRequired,
    read,
    command,
  };
}
export type LoginDeviceRemovalModel = ReturnType<typeof useLoginDeviceRemoval>;
