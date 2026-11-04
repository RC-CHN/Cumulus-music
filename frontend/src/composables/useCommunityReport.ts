import { shallowRef, type InjectionKey } from 'vue';
import type { CommunityReportClient } from '../services/community';
import type { ProfileReportChoice, ReportCommand } from '../services/profileReport';

export function useCommunityReport(core: CommunityReportClient, accountEpoch: () => string) {
  const target = shallowRef<{
    epoch: string;
    key: string;
    author: string;
    choice: ProfileReportChoice;
    opener: HTMLElement | undefined;
  }>();
  function open(key: string, author: string, choice: ProfileReportChoice, opener?: HTMLElement) {
    const epoch = accountEpoch();
    if (epoch && !target.value) target.value = { epoch, key, author, choice, opener };
  }
  function dispatch(epoch: string, key: string, command: ReportCommand) {
    if (command.kind === 'prepare') {
      if (command.reason.kind === 'meaningless')
        return Promise.reject(new Error('reason unavailable'));
      return core.communityNoteReport(epoch, key, { kind: 'prepare', reason: command.reason });
    }
    return core.communityNoteReport(epoch, key, command);
  }
  return {
    target,
    open,
    dispatch,
    close: () => (target.value = undefined),
  };
}
export const communityReportKey: InjectionKey<ReturnType<typeof useCommunityReport>> =
  Symbol('community-report');
