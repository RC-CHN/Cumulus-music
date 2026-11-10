import { computed, onScopeDispose, ref, shallowRef } from 'vue';
import { failure, type AccountData } from '../services/contracts.ts';
import {
  blockingProfileOperation,
  profileDraft,
  profileDraftChange,
  type AccountProfileEditClient,
  type ProfileDraft,
  type ProfileEditOperation,
  type ProfileEditCommand,
  type ProfileEditPage,
  type ProfileEditSnapshot,
} from '../services/accountProfileEdit.ts';

// Retain only the intent ID during this account epoch. A lost IPC reply may
// precede registration in native; an empty journal cannot authorize another write.
let unacknowledged: { epoch: string; id: string } | undefined;

export function useAccountProfileEdit(
  client: AccountProfileEditClient,
  owner: () => string | null,
  saved: (snapshot: ProfileEditSnapshot, confirmed: boolean) => void,
  expired: () => void,
) {
  const epoch = owner();
  if (unacknowledged?.epoch !== epoch) unacknowledged = undefined;
  const unseen = ref(unacknowledged?.id);
  const page = shallowRef<ProfileEditPage>();
  const baseline = shallowRef<ProfileEditSnapshot>();
  const draft = ref<ProfileDraft>();
  const busy = ref(false);
  const sending = ref(false);
  const loading = ref(false);
  const error = ref('');
  const notice = ref('');
  const refreshRequired = ref(false);
  const unavailableNickname = ref('');
  const nicknameTaken = computed(
    () => !!unavailableNickname.value && draft.value?.nickname.trim() === unavailableNickname.value,
  );
  let disposed = false;
  const active = () => !disposed && !!epoch && epoch === owner();
  const blocked = computed(() => blockingProfileOperation(page.value));
  const changes = computed(() =>
    baseline.value && draft.value
      ? profileDraftChange(profileDraft(baseline.value.profile), draft.value)
      : undefined,
  );
  const canSave = computed(
    () =>
      !busy.value &&
      !loading.value &&
      !unseen.value &&
      !blocked.value &&
      !refreshRequired.value &&
      !nicknameTaken.value &&
      !!changes.value?.valid &&
      changes.value.dirty,
  );
  function setBaseline(snapshot: ProfileEditSnapshot) {
    baseline.value = snapshot;
    draft.value = profileDraft(snapshot.profile);
  }
  function apply(response: AccountData<ProfileEditPage>) {
    if (!active()) return false;
    if (response.epoch !== epoch) {
      error.value = '读取结果与当前账号不匹配，请重新读取。';
      return false;
    }
    page.value = response.value;
    if (unseen.value && response.value.operations.some(({ id }) => id === unseen.value)) {
      if (unacknowledged?.id === unseen.value) unacknowledged = undefined;
      unseen.value = undefined;
    }
    return true;
  }
  function handleFailure(cause: unknown) {
    const { code } = failure(cause);
    if (code === 'session_expired' || code === 'profile_changed' || code === 'stale') {
      page.value = undefined;
      baseline.value = undefined;
      draft.value = undefined;
      expired();
    } else error.value = '暂时无法读取或核对个人资料，请重试。';
  }
  function handleOperation(op: ProfileEditOperation | undefined) {
    if (!op) return false;
    if (op.phase === 'confirmed' && op.observed) {
      setBaseline(op.observed);
      notice.value = '个人资料已保存。';
      saved(op.observed, true);
      return true;
    }
    if (op.phase === 'accepted_current_state' || op.phase === 'unchanged') {
      const snapshot = op.observed ?? page.value?.snapshot;
      if (snapshot) {
        setBaseline(snapshot);
        saved(snapshot, false);
      }
      notice.value = op.phase === 'unchanged' ? '当前资料已是所选内容。' : '已采用当前资料。';
    } else if (op.phase === 'rejected') {
      error.value = draft.value ? '修改尚未提交，草稿已保留。' : '此前的修改尚未提交。';
    }
    return false;
  }
  async function read(refresh = true) {
    if (!epoch || !active() || loading.value || busy.value) return;
    loading.value = true;
    error.value = '';
    const tracked = unseen.value ?? blocked.value?.id;
    try {
      if (refresh && !baseline.value) {
        if (!apply(await client.accountProfileEditPage(epoch, false))) return;
        if (tracked && handleOperation(page.value?.operations.find(({ id }) => id === tracked)))
          return;
        if (blocked.value || unseen.value) return;
      }
      if (!apply(await client.accountProfileEditPage(epoch, refresh))) return;
      const matching = page.value?.operations.find(({ id }) => id === tracked);
      if (handleOperation(matching)) return;
      if (
        (refresh || (!baseline.value && !!matching)) &&
        page.value?.snapshot &&
        !blocked.value &&
        !unseen.value
      ) {
        const previous = baseline.value && profileDraft(baseline.value.profile);
        const previousDraft = draft.value;
        setBaseline(page.value.snapshot);
        if (previous && previousDraft && draft.value && refreshRequired.value) {
          // Only reapply fields the user edited, never old untouched values.
          const preserve = <K extends keyof ProfileDraft>(key: K) => {
            if (previousDraft[key] !== previous[key]) draft.value![key] = previousDraft[key];
          };
          for (const key of ['nickname', 'signature', 'gender', 'birthday'] as const) preserve(key);
          if (
            previousDraft.province !== previous.province ||
            previousDraft.city !== previous.city
          ) {
            draft.value.province = previousDraft.province;
            draft.value.city = previousDraft.city;
          }
          notice.value = '已读取当前资料，请核对草稿后再保存。';
        }
        refreshRequired.value = false;
        return page.value.snapshot;
      } else if (!page.value?.snapshot && !blocked.value && !unseen.value) {
        error.value = '暂未读取到个人资料，请重试。';
      }
    } catch (cause) {
      if (active()) handleFailure(cause);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function command(command: ProfileEditCommand) {
    if (!epoch || !active() || loading.value || busy.value) return false;
    busy.value = true;
    error.value = '';
    notice.value = '';
    if (command.kind === 'change') {
      sending.value = true;
      unseen.value = command.id;
      unacknowledged = { epoch, id: command.id };
    }
    try {
      if (!apply(await client.accountProfileEditCommand(epoch, command))) return false;
      return handleOperation(page.value?.operations.find(({ id }) => id === command.id));
    } catch (cause) {
      if (!active()) return false;
      const { code } = failure(cause);
      if (
        command.kind === 'change' &&
        [
          'profile_nickname_taken',
          'profile_nickname_prohibited',
          'profile_nickname_unknown',
          'account_profile_stale',
          'invalid_input',
          'request_not_sent',
          'busy',
          'write_uncertain',
        ].includes(code)
      ) {
        if (unacknowledged?.id === command.id) unacknowledged = undefined;
        unseen.value = undefined;
        if (code === 'profile_nickname_taken' || code === 'profile_nickname_prohibited') {
          unavailableNickname.value = command.patch.nickname ?? '';
          error.value = code === 'profile_nickname_taken' ? '昵称已被占用。' : '此昵称不可使用。';
        } else if (code === 'profile_nickname_unknown') {
          error.value = '暂时无法核对昵称，草稿已保留。';
        } else if (code === 'account_profile_stale') {
          refreshRequired.value = true;
          error.value = '个人资料已更新。请先重新读取，草稿将保留。';
        } else if (code === 'busy' || code === 'write_uncertain') {
          refreshRequired.value = true;
          try {
            apply(await client.accountProfileEditPage(epoch, false));
          } catch {
            // This command never sent. Keep the draft until a current read.
          }
          if (active()) error.value = '请先核对正在处理的修改，草稿已保留。';
        } else if (code === 'request_not_sent') error.value = '修改尚未提交，草稿已保留。';
        else error.value = '修改内容不符合要求，草稿已保留。';
      } else if (code === 'account_profile_stale' && command.kind === 'accept_current') {
        // Native recorded a newer readback while refusing the acceptance.
        try {
          apply(await client.accountProfileEditPage(epoch, false));
        } catch {
          // Keep the unresolved operation and allow a fresh reconciliation.
        }
        if (active()) error.value = '个人资料已变化，请重新核对。';
      } else handleFailure(cause);
    } finally {
      if (active()) {
        busy.value = false;
        sending.value = false;
      }
    }
    return false;
  }
  function save() {
    if (!canSave.value || !baseline.value || !changes.value) return;
    return command({
      kind: 'change',
      id: crypto.randomUUID(),
      expectedRevision: baseline.value.revision,
      patch: changes.value.patch,
    });
  }
  function reconcile() {
    const op = blocked.value;
    if (op) return command({ kind: 'reconcile', id: op.id });
    return read(false);
  }
  function acceptCurrent() {
    const op = blocked.value;
    if (op?.observed)
      return command({ kind: 'accept_current', id: op.id, expectedRevision: op.observed.revision });
  }
  function refreshForAvatar() {
    refreshRequired.value = true;
    return read();
  }
  onScopeDispose(() => {
    disposed = true;
    baseline.value = undefined;
    draft.value = undefined;
    page.value = undefined;
  });
  return {
    avatarUrl: computed(() => baseline.value?.profile.avatarUrl),
    draft,
    blocked,
    unseen,
    busy,
    sending,
    loading,
    error,
    notice,
    changes,
    canSave,
    refreshRequired,
    nicknameTaken,
    read,
    save,
    reconcile,
    acceptCurrent,
    refreshForAvatar,
  };
}
