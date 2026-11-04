export type UpdateCheck =
  | { state: 'available' | 'no_newer'; currentVersion: string; latestVersion: string }
  | { state: 'no_published_release'; currentVersion: string };

export function updateFailure(reason: unknown): string {
  switch (reason) {
    case 'rate_limited':
      return 'GitHub 暂时限制了查询次数，请稍后重试。';
    case 'invalid_metadata':
      return '发布信息不完整或格式异常，暂时无法确认更新。';
    case 'busy':
      return '上一次检查还未结束，请稍后重试。';
    case 'network':
      return '无法连接 GitHub，请检查网络后重试。';
    case 'no_offer':
      return '此次检查的更新已失效，请重新检查。';
    case 'browser':
      return '无法打开系统浏览器，请重试。';
    default:
      return '暂时无法读取 Cumulus 发布信息，请稍后重试。';
  }
}
