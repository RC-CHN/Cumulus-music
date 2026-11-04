export type LoopbackCheck = 'available' | 'permission_denied' | 'unavailable';

export interface LocalNetworkReport {
  ipv4Loopback: LoopbackCheck;
  ipv6Loopback: LoopbackCheck;
  defaultRoute: 'present' | 'missing' | 'unavailable';
  proxyEnvironmentConfigured: boolean;
}
