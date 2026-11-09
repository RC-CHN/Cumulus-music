import { computed, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  DownloadClient,
  DownloadCollectCreation,
  DownloadCollectCreationCommand,
  DownloadCollectScope,
} from '../services/downloads.ts';
import { playlistPending, playlistSaved } from '../services/playlists.ts';

type Phase = 'prepared' | 'pending' | 'created_ready' | 'failed' | 'complete';

export function useDownloadCollectCreation(
  core: Pick<DownloadClient, 'downloadCollectCreation'>,
  current: () => DownloadCollectScope | undefined,
  changed: () => void,
  recorded: () => void,
  expired: () => void,
) {
  const phase = shallowRef<Phase>('prepared');
  const busy = shallowRef(false);
  const notice = shallowRef('');
  let last: DownloadCollectCreation | undefined;
  const reported = new Set<string>();

  function settle(value: DownloadCollectCreation, scope: DownloadCollectScope) {
    const { create, add } = value;
    if (
      value.receipt !== scope.receipt ||
      create.kind !== 'create' ||
      (last &&
        (create.operationId !== last.create.operationId ||
          (last.create.playlistId && create.playlistId !== last.create.playlistId) ||
          (last.add && add?.operationId !== last.add.operationId))) ||
      (add &&
        (add.kind !== 'tracks' || !create.playlistId || add.playlistId !== create.playlistId)) ||
      (value.canAdd && (add || !create.playlistId || create.phase !== 'confirmed'))
    ) {
      phase.value = 'pending';
      notice.value = '无法确认这次歌单操作，请核对结果。';
      return;
    }
    last = value;
    let updated = false;
    for (const entry of [create, add]) {
      if (!entry || !playlistSaved(entry.phase) || reported.has(entry.operationId)) continue;
      reported.add(entry.operationId);
      updated = true;
    }
    if (updated) changed();
    recorded();
    if (add && playlistSaved(add.phase)) {
      phase.value = 'complete';
      notice.value = '已收藏到新歌单。';
    } else if (playlistPending(create.phase) || (add && playlistPending(add.phase))) {
      phase.value = 'pending';
      notice.value = add ? '收藏结果待核对。' : '创建歌单结果待核对。';
    } else if (value.canAdd) {
      phase.value = 'created_ready';
      notice.value = '歌单已创建，可以继续收藏这首歌。';
    } else {
      phase.value = 'failed';
      notice.value =
        create.phase === 'confirmed'
          ? add
            ? '歌单已创建，但未能收藏这首歌。'
            : '已观察到新歌单，但缺少创建回执，未收藏歌曲。'
          : '未能创建歌单，未收藏歌曲。';
    }
  }

  async function act(from: Phase, command: DownloadCollectCreationCommand) {
    const owner = current();
    if (!owner?.canCreate || busy.value || phase.value !== from) return;
    const live = () => current() === owner;
    busy.value = true;
    notice.value =
      command.kind === 'status'
        ? '正在核对结果…'
        : command.kind === 'create'
          ? '正在创建并收藏…'
          : '正在收藏…';
    try {
      let result;
      try {
        result = await core.downloadCollectCreation(owner.epoch, owner.receipt, command);
      } catch (error) {
        if (
          command.kind === 'create' &&
          live() &&
          failure(error).code === 'download_create_not_started'
        ) {
          // The native visit was released before dispatch. Do not poll a
          // nonexistent journal or resubmit until the user confirms again.
          notice.value = failure(error).message;
          return;
        }
        if (!live() || command.kind === 'status') throw error;
        // Recover a lost IPC response from the same receipt's local journal.
        // Only a later explicit reconcile may ask the service to read back.
        try {
          result = await core.downloadCollectCreation(owner.epoch, owner.receipt, {
            kind: 'status',
            reconcile: false,
          });
        } catch {
          throw error;
        }
      }
      if (!live()) return;
      if (result.epoch !== owner.epoch) throw new Error('这次歌单操作已失效。');
      settle(result.value, owner);
    } catch (error) {
      if (!live()) return;
      const issue = failure(error);
      if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
      if (!live()) return;
      phase.value = [
        'invalid_input',
        'stale',
        'auth_required',
        'session_expired',
        'profile_changed',
      ].includes(issue.code)
        ? 'failed'
        : 'pending';
      notice.value = issue.message;
    } finally {
      if (live()) busy.value = false;
    }
  }

  return {
    busy,
    phase,
    notice,
    locked: computed(() => busy.value || phase.value !== 'prepared'),
    create: (name: string, privatePlaylist: boolean) =>
      act('prepared', { kind: 'create', name, privatePlaylist }),
    reconcile: () => act('pending', { kind: 'status', reconcile: true }),
    continueAdd: () => act('created_ready', { kind: 'add_created' }),
  };
}
