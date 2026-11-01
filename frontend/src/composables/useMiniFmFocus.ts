import { nextTick, onScopeDispose, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import type { FmModel } from './useFm';

/** Wait for both the actual FM read and the page transition; keep user focus. */
export function useMiniFmFocus(model: FmModel, account: () => AccountView, page: () => string) {
  let wanted: { epoch: string; focus: Element | null } | undefined;
  function request(epoch: string) {
    wanted = { epoch, focus: document.activeElement };
  }
  function complete() {
    if (!wanted) return;
    if (
      account().epoch !== wanted.epoch ||
      account().status !== 'authenticated' ||
      page() !== 'fm'
    ) {
      wanted = undefined;
      return;
    }
    if (model.loading.value) return;
    const root = document.querySelector<HTMLElement>('.fm-page');
    const target =
      root?.querySelector<HTMLElement>('.fm-recovery') ??
      root?.querySelector<HTMLElement>('.fm-state-region') ??
      root?.querySelector<HTMLElement>('h1');
    if (!target || target.closest('.page-enter-active, .page-leave-active')) return;
    const original = wanted.focus;
    wanted = undefined;
    if (
      ![original, document.body, target].includes(document.activeElement) ||
      target.closest('[inert]')
    )
      return;
    target.focus({ preventScroll: true });
    target.scrollIntoView({ block: 'nearest' });
  }
  watch(
    [
      model.loading,
      model.view,
      model.stateIssue,
      page,
      () => account().epoch,
      () => account().status,
    ],
    async () => {
      await nextTick();
      complete();
    },
  );
  onScopeDispose(() => {
    wanted = undefined;
  });
  return { request, complete };
}
