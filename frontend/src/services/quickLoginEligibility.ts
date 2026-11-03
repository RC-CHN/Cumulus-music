import type { AccountData } from './contracts.ts';

export interface QuickLoginEligibilityAccount {
  id: string;
  current: boolean;
  eligible: boolean;
}

export interface QuickLoginEligibilityView {
  accounts: QuickLoginEligibilityAccount[];
  checkedCount: number;
}

export interface QuickLoginEligibilityClient {
  quickLoginEligibility(epoch: string): Promise<AccountData<QuickLoginEligibilityView>>;
}
