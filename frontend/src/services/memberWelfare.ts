import type { AccountData } from './contracts';

export interface MemberWelfareView {
  rightTips: string | null;
  unreadCount: number;
  items: Array<{ name: string; description: string | null; unread: boolean }>;
}

export interface MemberWelfareClient {
  memberWelfareAvailable(epoch: string): Promise<AccountData<boolean>>;
  memberWelfareList(epoch: string): Promise<AccountData<MemberWelfareView | null>>;
}
