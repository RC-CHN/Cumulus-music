import type { AccountBinding } from './accountDetail.ts';

const order = ['cellphone', 'email', 'wechat', 'qq', 'weibo'] as const;
export type BindingMethod = (typeof order)[number];
export interface AccountBindingsView {
  registrationProvider: BindingMethod | null;
  foreignRegisteredPhone: boolean;
  bindings: AccountBinding[];
}

export const bindingMethodName: Record<BindingMethod, string> = {
  cellphone: '手机',
  email: '网易邮箱账号',
  wechat: '微信',
  qq: 'QQ',
  weibo: '新浪微博',
};

interface BindingRow {
  method: BindingMethod;
  registered: boolean;
  binding?: AccountBinding;
  detail?: string;
  change?: string;
  password?: string;
  mayBind: boolean;
}

export function bindingGroups(view: AccountBindingsView) {
  const registration = view.registrationProvider;
  const rows: BindingRow[] = order.map((method) => {
    const binding = view.bindings.find((row) => row.provider === method);
    const registered = method === registration;
    const address = binding?.maskedAddress || binding?.displayName;
    const phone = method === 'cellphone';
    let detail: string | undefined;
    if (address) {
      if (phone) {
        const number = binding?.countryCode ? `+${binding.countryCode} ${address}` : address;
        detail = `${registered ? '使用手机号注册' : '手机号'}：${number}`;
      } else {
        detail = method === 'email' || registered ? address : `ID：${address}`;
      }
    }
    return {
      method,
      binding,
      registered,
      detail,
      change:
        binding && detail
          ? phone
            ? registered
              ? '更换手机'
              : '更换手机号'
            : registration && !registered
              ? '解绑账号'
              : undefined
          : undefined,
      password:
        phone && binding?.hasPassword != null
          ? binding.hasPassword
            ? '修改密码'
            : '设置密码'
          : undefined,
      mayBind: !registered && !binding,
    };
  });
  return registration
    ? [
        { key: 'registration', title: '注册账号', rows: rows.filter((row) => row.registered) },
        { key: 'others', title: '其他登录方式', rows: rows.filter((row) => !row.registered) },
      ]
    : [{ key: 'unclassified', title: '登录方式', rows }];
}
