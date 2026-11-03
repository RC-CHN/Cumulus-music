import type { SelfProfile } from './accountDetail.ts';
import type { AccountData } from './contracts.ts';
import { nicknameFormatIssue, nicknameLength } from './nickname.ts';

export interface ProfileBirthday {
  year: number;
  month: number;
  day: number;
  utcOffsetMinutes: number;
}

export interface ProfilePatch {
  nickname?: string;
  signature?: string;
  gender?: 1 | 2;
  birthday?: ProfileBirthday;
  region?: { province: number; city: number };
}

export interface ProfileEditSnapshot {
  revision: string;
  fetchedAt: number;
  profile: SelfProfile;
}

export interface ProfileEditOperation {
  id: string;
  expectedRevision: string;
  patch: ProfilePatch;
  submitted: ProfilePatch;
  phase:
    | 'pending'
    | 'confirmed'
    | 'rejected'
    | 'uncertain'
    | 'unchanged'
    | 'accepted_current_state';
  createdAt: number;
  observed: ProfileEditSnapshot | null;
}

export interface ProfileEditPage {
  snapshot: ProfileEditSnapshot | null;
  operations: ProfileEditOperation[];
}

export type ProfileEditCommand =
  | { kind: 'change'; id: string; expectedRevision: string; patch: ProfilePatch }
  | { kind: 'reconcile'; id: string }
  | { kind: 'accept_current'; id: string; expectedRevision: string };

export interface AccountProfileEditClient {
  accountProfileEditPage(epoch: string, refresh: boolean): Promise<AccountData<ProfileEditPage>>;
  accountProfileEditCommand(
    epoch: string,
    command: ProfileEditCommand,
  ): Promise<AccountData<ProfileEditPage>>;
}

export interface ProfileDraft {
  nickname: string;
  signature: string;
  gender: 1 | 2 | '';
  birthday: string;
  province: number | '';
  city: number | '';
}

function dateInputValue(date: Date) {
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function profileDraft(profile: SelfProfile): ProfileDraft {
  const date = profile.birthday ? new Date(profile.birthday) : null;
  return {
    nickname: profile.nickname ?? '',
    signature: profile.signature ?? '',
    gender: profile.gender === 1 || profile.gender === 2 ? profile.gender : '',
    birthday: date && Number.isFinite(date.getTime()) ? dateInputValue(date) : '',
    province: profile.province || '',
    city: profile.city || '',
  };
}

export function localProfileBirthday(
  value: string,
  today = new Date(),
): ProfileBirthday | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return;
  const [year, month, day] = match.slice(1).map(Number) as [number, number, number];
  const date = new Date(year, month - 1, day);
  if (
    year < 1920 ||
    value > dateInputValue(today) ||
    date.getTime() === 0 ||
    date.getFullYear() !== year ||
    date.getMonth() + 1 !== month ||
    date.getDate() !== day
  )
    return;
  const offset = -date.getTimezoneOffset();
  if (Math.abs(offset) > 14 * 60) return;
  return { year, month, day, utcOffsetMinutes: offset || 0 };
}

export const profileBirthdayToday = () => dateInputValue(new Date());

export function profileDraftChange(base: ProfileDraft, draft: ProfileDraft) {
  const patch: ProfilePatch = {};
  const issues: Partial<Record<'nickname' | 'signature' | 'birthday' | 'region', string>> = {};
  if (draft.nickname.trim() !== base.nickname.trim()) {
    switch (nicknameFormatIssue(draft.nickname)) {
      case 'too_short':
        issues.nickname = '昵称至少需要 4 个字符';
        break;
      case 'too_long':
        issues.nickname = '昵称最多可用 30 个字符';
        break;
      case 'invalid_characters':
        issues.nickname = '昵称只支持汉字、字母、数字、下划线和短横线';
        break;
      default:
        patch.nickname = draft.nickname.trim();
    }
  }
  if (draft.signature !== base.signature) {
    if (nicknameLength(draft.signature) > 300) issues.signature = '简介最多可用 300 个字符';
    else if (
      Array.from(draft.signature).some((char) => {
        const code = char.charCodeAt(0);
        return (code < 32 && code !== 10) || (code >= 127 && code <= 159);
      })
    )
      issues.signature = '简介含有不支持的控制字符';
    else patch.signature = draft.signature;
  }
  if (draft.gender && draft.gender !== base.gender) patch.gender = draft.gender;
  if (draft.birthday !== base.birthday) {
    const birthday = localProfileBirthday(draft.birthday);
    if (birthday) patch.birthday = birthday;
    else issues.birthday = '请选择有效的生日';
  }
  if (draft.province !== base.province || draft.city !== base.city) {
    if (draft.province && draft.city) patch.region = { province: draft.province, city: draft.city };
    else issues.region = '请选择完整的地区';
  }
  return {
    patch,
    issues,
    dirty: Object.keys(patch).length > 0 || Object.keys(issues).length > 0,
    valid: Object.keys(issues).length === 0,
  };
}

export function blockingProfileOperation(page: ProfileEditPage | undefined) {
  return page?.operations.find(({ phase }) => phase === 'pending' || phase === 'uncertain');
}
