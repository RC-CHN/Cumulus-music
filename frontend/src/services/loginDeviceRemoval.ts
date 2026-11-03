import type { AccountData } from './contracts.ts';

export type LoginDeviceRemovalPhase =
  | 'needs_binding'
  | 'needs_sms'
  | 'sms_ready'
  | 'sms_send_uncertain'
  | 'recovery_required'
  | 'observed_absent'
  | 'not_sent'
  | 'cancelled';

export interface LoginDeviceRemovalView {
  id: string;
  phase: LoginDeviceRemovalPhase;
  phoneTail: string | null;
  resendAfterSeconds: number;
  smsNotAccepted: boolean;
  observedPresent: boolean | null;
}

export type LoginDeviceRemovalCommand =
  | { kind: 'begin'; device_id: string }
  | { kind: 'resolve_begin'; device_id: string }
  | { kind: 'submit_sms'; operation_id: string; code: string }
  | { kind: 'send_sms' | 'reconcile' | 'cancel'; operation_id: string };

export interface LoginDeviceRemovalClient {
  accountLoginDeviceRemoval(epoch: string): Promise<AccountData<LoginDeviceRemovalView | null>>;
  accountLoginDeviceRemovalCommand(
    epoch: string,
    command: LoginDeviceRemovalCommand,
  ): Promise<AccountData<LoginDeviceRemovalView>>;
}

export function removalSms(phase: LoginDeviceRemovalPhase | undefined) {
  return phase === 'needs_sms' || phase === 'sms_ready' || phase === 'sms_send_uncertain';
}

export function removalPending(phase: LoginDeviceRemovalPhase | undefined) {
  return removalSms(phase) || phase === 'recovery_required';
}

export function validDeviceRemovalCode(code: string) {
  return /^[0-9]{4,8}$/.test(code);
}
