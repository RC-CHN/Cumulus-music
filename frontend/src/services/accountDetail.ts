import type { AccountData } from './contracts';
import regions from '../../../core/assets/account-regions.json' with { type: 'json' };

export const accountRegions: readonly {
  id: number;
  name: string;
  type?: number;
  cities: Readonly<Record<string, string | undefined>>;
}[] = regions;
const regionIndex = new Map(accountRegions.map((region) => [region.id, region]));

export function accountRegion(province?: number | null, city?: number | null) {
  const region = province ? regionIndex.get(province) : undefined;
  const cityName = region && city ? region.cities[city] : undefined;
  if (cityName && (region?.type === 1 || region?.type === 2)) return cityName;
  return [
    province ? (region?.name ?? `省份代码 ${province}`) : '',
    city ? (cityName ?? `城市代码 ${city}`) : '',
  ]
    .filter(Boolean)
    .join(' · ');
}

export interface SelfProfile {
  uid: string;
  nickname?: string | null;
  avatarUrl?: string | null;
  signature?: string | null;
  gender?: number | null;
  birthday?: number | null;
  province?: number | null;
  city?: number | null;
  level?: number | null;
}
export interface AccountBinding {
  provider: 'email' | 'cellphone' | 'weibo' | 'qq' | 'wechat' | 'unknown';
  typeCode: number;
  displayName?: string | null;
  maskedAddress?: string | null;
  countryCode?: string | null;
  hasPassword?: boolean | null;
  detailsAvailable: boolean;
}
export interface AccountProfileDetail {
  profile: SelfProfile;
  bindings: AccountBinding[];
}
export interface LoginDevice {
  id: string;
  name?: string | null;
  location?: string | null;
  loginType?: string | null;
  lastActiveTime?: number | null;
  os?: string | null;
  current: boolean;
  signedIn: boolean;
}
export interface LoginDevicesView {
  devices: LoginDevice[];
  limits?: { currentNum?: number | null; deviceLimit?: number | null } | null;
}
export interface AccountBindingQrView {
  modules: boolean[][];
}
export interface AccountDetailClient {
  accountProfileDetail(epoch: string): Promise<AccountData<AccountProfileDetail>>;
  accountLoginDevices(epoch: string): Promise<AccountData<LoginDevicesView>>;
  accountBindingQr(epoch: string): Promise<AccountData<AccountBindingQrView>>;
}

export function accountDate(value: number | null | undefined, time = false): string | null {
  if (value == null || value === 0 || !Number.isSafeInteger(value)) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  const pad = (part: number) => String(part).padStart(2, '0');
  const day = `${date.getFullYear()}.${pad(date.getMonth() + 1)}.${pad(date.getDate())}`;
  return time
    ? `${day} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
    : day;
}

export function accountGender(value: number | null | undefined): string {
  return value === 1 ? '男' : value === 2 ? '女' : '未提供';
}

export function bindingLabel(provider: AccountBinding['provider']): string {
  switch (provider) {
    case 'cellphone':
      return '手机号';
    case 'email':
      return '邮箱';
    case 'wechat':
      return '微信';
    case 'qq':
      return 'QQ';
    case 'weibo':
      return '微博';
    default:
      return '其他绑定';
  }
}

export function groupLoginDevices(devices: LoginDevice[]) {
  const signedIn = devices.filter((device) => device.signedIn === true);
  return {
    signedIn: [
      ...signedIn.filter((device) => device.current),
      ...signedIn.filter((device) => !device.current),
    ],
    signedOut: devices.filter((device) => device.signedIn === false),
  };
}

export function loginDeviceIcon(os: string | null | undefined): string {
  switch (os?.toLowerCase()) {
    case 'android':
    case 'ios':
    case 'iphone':
    case 'ipad':
      return 'phone';
    default:
      return 'monitor';
  }
}
