import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  penaltyInstruction,
  penaltyStepLabel,
  type PenaltyView,
} from '../src/services/loginPenalty.ts';
const base: PenaltyView = {
  id: 'synthetic-id',
  phase: 'required',
  code: 8831,
  steps: [],
  currentStep: null,
  boundary: 'official_account_verification',
  canCheckStatus: true,
};
test('status-passed guidance does not claim authenticated login or encourage old password replay', () => {
  assert.match(penaltyInstruction({ ...base, phase: 'status_passed' }), /尚需重新登录/);
  assert.match(
    penaltyInstruction({ ...base, boundary: 'official_password_upgrade' }),
    /新密码重新登录/,
  );
});
test('every recovered or unknown pending step remains an external official-client action', () => {
  const boundaries: PenaltyView['boundary'][] = [
    'official_account_verification',
    'official_identity_verification',
    'official_bind_phone',
    'official_real_name',
    'official_password_upgrade',
    'official_version_upgrade',
    'unsupported_step',
  ];
  for (const boundary of boundaries)
    assert.match(penaltyInstruction({ ...base, boundary }), /官方客户端/);
  assert.equal(
    penaltyStepLabel({
      kind: 'unknown',
      typeName: 'https://example.invalid/never-open',
      passed: false,
      minVersion: null,
    }),
    '其他账号验证',
  );
});
