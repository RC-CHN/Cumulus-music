export interface LoginCountry {
  alpha: string;
  name: string;
  englishName: string;
  calling: string;
  allowedLengths: number[];
}
export interface LoginPresentation {
  countries: LoginCountry[];
  selectedCountry: string;
}
export type LoginDocument = 'service' | 'privacy' | 'children' | 'email_recovery';

export interface LoginPresentationClient {
  loginPresentation(): Promise<LoginPresentation>;
  selectLoginCountry(alpha: string): Promise<void>;
  openLoginDocument(document: LoginDocument): Promise<void>;
}
const countryWrites = new WeakMap<LoginPresentationClient, Promise<void>>();
export function saveLoginCountry(core: LoginPresentationClient, alpha: string): Promise<void> {
  const next = (countryWrites.get(core) ?? Promise.resolve())
    .catch(() => {})
    .then(() => core.selectLoginCountry(alpha));
  countryWrites.set(core, next);
  return next;
}
export async function loadLoginPresentation(
  core: LoginPresentationClient,
): Promise<LoginPresentation> {
  await countryWrites.get(core)?.catch(() => {});
  return core.loginPresentation();
}

export function phoneError(rows: LoginCountry[], phone: string, calling: string): string {
  if (!phone) return '请输入手机号';
  const row = rows.find((row) => row.calling === calling);
  if (!row) return '请选择国际区号';
  if (
    !/^[0-9]{1,32}$/.test(phone) ||
    (row.allowedLengths.length > 0 && !row.allowedLengths.includes(phone.length))
  )
    return row.allowedLengths.length
      ? `请输入${row.allowedLengths.join('或')}位数字的手机号`
      : '请输入正确的手机号';
  return '';
}

const emailSuffixes = [
  '@163.com',
  '@126.com',
  '@yeah.net',
  '@vip.163.com',
  '@vip.126.com',
  '@188.com',
];
export function emailSuggestions(value: string): string[] {
  if (!value || /\s/.test(value) || value.split('@').length > 2) return [];
  const [name = '', suffix] = value.split('@');
  if (!name) return [];
  // Literal matching avoids treating user input as the source's search regex.
  return emailSuffixes
    .filter((candidate) => suffix === undefined || candidate.includes(suffix))
    .map((candidate) => name + candidate)
    .filter((candidate) => candidate.length <= 254 && candidate !== value);
}
