import { ref, shallowRef } from 'vue';
import type {
  MemberClient,
  MemberSignCard,
  MemberSignDay,
  MemberSignDetail,
} from '../services/member.ts';

export function useMemberSign(core: MemberClient, describeError: (e: unknown) => string) {
  const card = shallowRef<MemberSignCard>();
  const day = shallowRef<MemberSignDay>();
  const detail = shallowRef<MemberSignDetail>();
  const cardBusy = ref(false),
    detailBusy = ref(false),
    cardError = ref(''),
    detailError = ref('');
  let epoch: string | undefined,
    visible = false,
    cardSerial = 0,
    detailSerial = 0;

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
    card.value = undefined;
    cardBusy.value = false;
    cardError.value = '';
    close();
    if (visible) void load();
  }

  async function load() {
    if (!epoch || !visible || cardBusy.value) return;
    const owner = epoch,
      serial = ++cardSerial;
    const active = () => owner === epoch && visible && serial === cardSerial;
    cardBusy.value = true;
    cardError.value = '';
    try {
      const reply = await core.memberSignCard(owner);
      if (active() && reply.epoch === owner) card.value = reply.value;
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
    if (!epoch || !visible || !selected.signed || !selected.signTime || detailBusy.value) return;
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
    cardBusy,
    detailBusy,
    cardError,
    detailError,
    connect,
    show,
    load,
    open,
    close,
  };
}
