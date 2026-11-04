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

export interface PackageVerification {
  version: string;
  filename: string;
  format: 'linux_deb' | 'linux_portable' | 'windows_zip' | 'windows_setup';
}

export function packageFailure(reason: unknown): string {
  switch (reason) {
    case 'no_metadata':
      return '没有本次发布的校验信息，请先重新检查更新。';
    case 'wrong_name':
      return '文件名与本次发布的安装包不匹配，请选择保留原文件名的安装包。';
    case 'wrong_size':
      return '文件大小与发布记录不一致。';
    case 'wrong_format':
      return '文件格式与发布记录不一致。';
    case 'digest_mismatch':
      return 'SHA-256 与发布记录不一致。请勿使用这个安装包。';
    case 'changed':
      return '文件在校验时发生了变化，请重新选择。';
    case 'invalid_file':
      return '请选择已保存在本机的普通文件。';
    case 'too_large':
      return '这个文件超过本机校验支持的大小。';
    case 'busy':
      return '另一项更新操作正在进行，请稍后重试。';
    default:
      return '无法读取文件，请重新选择。';
  }
}
