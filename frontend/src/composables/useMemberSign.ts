import { ref, shallowRef } from 'vue';
import type {
  MemberClient,
  MemberSignAction,
  MemberSignCard,
  MemberSignCommand,
  MemberSignDay,
  MemberSignDetail,
} from '../services/member.ts';

export function useMemberSign(core: MemberClient, describeError: (e: unknown) => string) {
  const card = shallowRef<MemberSignCard>();
  const day = shallowRef<MemberSignDay>();
  const detail = shallowRef<MemberSignDetail>();
  const action = shallowRef<MemberSignAction>();
  const confirmedDetail = shallowRef<MemberSignDetail>();
  const cardBusy = ref(false),
    detailBusy = ref(false),
    signBusy = ref(false),
    cardError = ref(''),
    detailError = ref(''),
    signError = ref('');
  let epoch: string | undefined,
    visible = false,
    cardSerial = 0,
    detailSerial = 0,
    signSerial = 0;

  function observe(next: MemberSignAction, result?: MemberSignDetail | null) {
    if (next.phase !== 'signed' || next.dayKey !== action.value?.dayKey)
      confirmedDetail.value = undefined;
    action.value = next;
    if (next.phase === 'signed' && result) confirmedDetail.value = result;
  }

  function close() {
    detailSerial++;
    day.value = undefined;
    detail.value = undefined;
    detailBusy.value = false;
    detailError.value = '';
  }

  function connect(next: string | undefined) {
    if (epoch === next) return;
    epoch = next;
    cardSerial++;
    signSerial++;
    card.value = undefined;
    action.value = undefined;
    confirmedDetail.value = undefined;
    cardBusy.value = false;
    signBusy.value = false;
    cardError.value = '';
    signError.value = '';
    close();
    if (visible) void load();
  }

  async function load() {
    if (!epoch || !visible || cardBusy.value || signBusy.value) return;
    const owner = epoch,
      serial = ++cardSerial;
    const active = () => owner === epoch && visible && serial === cardSerial;
    cardBusy.value = true;
    cardError.value = '';
    try {
      const reply = await core.memberSignCard(owner);
      if (active() && reply.epoch === owner) {
        card.value = reply.value;
        observe(reply.value.action);
        signError.value = '';
      }
    } catch (e) {
      if (active()) {
        const message = describeError(e);
        if (active()) cardError.value = message;
      }
    } finally {
      if (active()) cardBusy.value = false;
    }
  }

  async function open(selected: MemberSignDay) {
    if (
      !epoch ||
      !visible ||
      !selected.signed ||
      !selected.signTime ||
      detailBusy.value ||
      signBusy.value
    )
      return;
    const owner = epoch,
      serial = ++detailSerial;
    const active = () => owner === epoch && visible && serial === detailSerial;
    day.value = selected;
    detail.value = undefined;
    detailBusy.value = true;
    detailError.value = '';
    try {
      const reply = await core.memberSignDetail(owner, selected.signTime);
      if (active() && reply.epoch === owner) detail.value = reply.value;
    } catch (e) {
      if (active()) {
        const message = describeError(e);
        if (active()) detailError.value = message;
      }
    } finally {
      if (active()) detailBusy.value = false;
    }
  }

  function showConfirmedDetail() {
    const result = confirmedDetail.value;
    if (!visible || action.value?.phase !== 'signed' || !result || day.value) return;
    detail.value = result;
    day.value = {
      label: '今天',
      today: true,
      signed: true,
      signTime: result.time,
      coverUrl: result.coverUrl,
    };
  }

  async function submit(command: MemberSignCommand) {
    const displayed = action.value;
    if (!epoch || !visible || signBusy.value || cardBusy.value || day.value || !displayed) return;
    if (command.kind === 'check_in') {
      if (
        displayed.phase !== 'ready' ||
        !command.dayKey ||
        displayed.dayKey !== command.dayKey ||
        cardError.value
      )
        return;
    } else if (displayed.phase !== 'unknown') return;
    const owner = epoch;
    const serial = ++signSerial;
    const active = () => owner === epoch && serial === signSerial;
    signBusy.value = true;
    signError.value = '';
    try {
      const reply = await core.memberSignCommand(owner, command);
      if (!active()) return;
      if (reply.epoch !== owner) throw { code: 'stale', message: '账号已切换，请重新读取乐签' };
      const result = reply.value;
      if (result.card) {
        card.value = result.card;
        cardError.value = '';
      }
      observe(result.action, result.detail);
      showConfirmedDetail();
    } catch (e) {
      if (active()) {
        // A lost IPC reply is not proof the write failed. Retire the displayed
        // write action until the native account journal is read again.
        observe({ phase: 'unknown', dayKey: displayed.dayKey });
        const message = describeError(e);
        if (active()) signError.value = message;
      }
    } finally {
      if (active()) signBusy.value = false;
    }
  }

  function show(value: boolean) {
    if (value === visible) return;
    visible = value;
    if (value) void load();
    else {
      cardSerial++;
      cardBusy.value = false;
      close();
    }
  }

  return {
    card,
    day,
    detail,
    action,
    confirmedDetail,
    cardBusy,
    detailBusy,
    signBusy,
    cardError,
    detailError,
    signError,
    connect,
    show,
    load,
    open,
    submit,
    showConfirmedDetail,
    close,
  };
}
