/** Visibility/focus repair is a read of the existing native actor, never a
 * replay of playback, a source refresh, or a retry of a previous user command. */
export function createPlayerResume(
  player: { ready: { readonly value: boolean }; syncPlayback(): Promise<unknown> },
  visible: () => boolean,
) {
  let alive = true,
    again = false,
    pending: Promise<void> | undefined;
  const usable = () => alive && player.ready.value && visible();
  async function drain() {
    do {
      again = false;
      try {
        await player.syncPlayback();
      } catch {
        /* The existing stream/recovery UI owns connectivity errors. */
      }
    } while (again && usable());
  }
  function wake(): Promise<void> {
    if (!usable()) return Promise.resolve();
    if (pending) {
      again = true;
      return pending;
    }
    pending = drain().finally(() => {
      pending = undefined;
    });
    return pending;
  }
  return {
    wake,
    dispose: () => {
      alive = false;
      again = false;
    },
  };
}
