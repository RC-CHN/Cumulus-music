import { watch, type Ref } from 'vue';

// These confirmations replace content inside an already-open native dialog.
// showModal() cannot move focus again when the focused entry is removed.
export function useConfirmationFocus(
  dialog: Ref<HTMLDialogElement | undefined>,
  step: Readonly<Ref<string | undefined>>,
) {
  watch(
    step,
    (current, previous) => {
      const root = dialog.value;
      if (!root?.isConnected || !root.open) return;
      const entry = [...root.querySelectorAll<HTMLButtonElement>('[data-confirmation-entry]')].find(
        (node) => node.dataset.confirmationEntry === previous && !node.disabled,
      );
      const target = current
        ? root.querySelector<HTMLButtonElement>('[data-confirmation-cancel]')
        : (entry ?? root.querySelector<HTMLButtonElement>('[data-confirmation-fallback]'));
      if (!target || target.disabled) return;
      target.focus({ preventScroll: true });
      target.scrollIntoView({ block: 'nearest' });
    },
    { flush: 'post' },
  );
}
