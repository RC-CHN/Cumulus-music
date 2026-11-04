import type { AccountData } from './contracts';

export const profileReportChoices = [
  ['advertising', '广告或营销信息'],
  ['personal_attack', '恶意人身攻击'],
  ['harmful_content', '色情、低俗或有害内容'],
  ['political', '政治敏感言论'],
  ['other', '其他原因'],
] as const;
export type ProfileReportChoice = (typeof profileReportChoices)[number][0];
type Presets<Choice extends string> = { [Kind in Choice]: { kind: Kind } }[Choice];
export type ProfileReportReason =
  | Presets<Exclude<ProfileReportChoice, 'other'>>
  | { kind: 'other'; description: string };
export type ReportChoice = ProfileReportChoice | 'meaningless';
export type ReportReason = ProfileReportReason | { kind: 'meaningless' };
export const maxReportDescriptionBytes = 4096;

export type ReportCommand<Reason = ReportReason> =
  | { kind: 'inspect' }
  | { kind: 'prepare'; reason: Reason }
  | { kind: 'cancel' | 'submit'; receipt: string };

export type ProfileReportCommand = ReportCommand<ProfileReportReason>;
export type ProfileReportStatus =
  | 'ready'
  | 'accepted'
  | 'duplicate'
  | 'official_account'
  | 'uncertain'
  | 'not_sent'
  | 'target_unavailable';
export interface ProfileReportView {
  status: ProfileReportStatus;
  receipt: string | null;
}
export interface ProfileReportClient {
  profileReport(
    epoch: string,
    uid: string,
    command: ProfileReportCommand,
  ): Promise<AccountData<ProfileReportView>>;
}
