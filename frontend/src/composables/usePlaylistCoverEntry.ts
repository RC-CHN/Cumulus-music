import { computed, ref } from 'vue';
import type { AccountView, CoreClient } from '../services/contracts.ts';
import type { PlaylistEditor } from '../services/playlists.ts';
import type { CoverOutcome } from '../services/playlistCover.ts';
import { usePlaylistCover } from './usePlaylistCover.ts';

/** Recover existing work before asking for a new file; open the cropper only
 * after selection or a visible error. Native picker cancellation stays on page. */
export function usePlaylistCoverEntry(
  account: () => AccountView,
  editor: () => PlaylistEditor | undefined,
  saved: (outcome: CoverOutcome) => Promise<void>,
  expired: () => void,
  returnFocus: () => Promise<void>,
  core?: CoreClient,
) {
  const model = usePlaylistCover(core, saved, expired);
  const open = ref(false),
    phase = ref<'idle' | 'reading' | 'picking'>('idle'),
    notice = ref('');
  const starting = computed(() => phase.value !== 'idle');
  let alive = true,
    serial = 0;
  async function start(recover = false) {
    const owner = account(),
      target = editor();
    if (
      !alive ||
      starting.value ||
      open.value ||
      owner.status !== 'authenticated' ||
      !target?.owned ||
      target.favorite
    )
      return;
    const turn = ++serial;
    const live = () =>
      alive && turn === serial && account().epoch === owner.epoch && editor()?.id === target.id;
    phase.value = 'reading';
    notice.value = '';
    let show = false;
    try {
      if (!(await model.connect(owner, target.id)) || !live()) return;
      if (!model.task.value && !model.error.value && !model.readFailed.value) {
        if (recover) {
          notice.value = '没有需要恢复的封面编辑，可以重新选择图片。';
        } else {
          phase.value = 'picking';
          if ((await model.choose(target.revision)) === undefined || !live()) return;
        }
      }
      show = !!model.task.value || !!model.error.value || model.readFailed.value;
    } finally {
      if (alive && turn === serial) phase.value = 'idle';
    }
    if (!live()) return;
    // Enable the fieldset before restoring focus. ModalTransition can then
    // capture the cover trigger instead of the OS chooser's lost focus.
    await returnFocus();
    if (live()) open.value = show;
  }
  function dispose() {
    alive = false;
    serial++;
    open.value = false;
    phase.value = 'idle';
    model.dispose();
  }
  return {
    model,
    open,
    phase,
    starting,
    notice,
    start,
    dismiss: () => {
      open.value = false;
    },
    dispose,
  };
}
