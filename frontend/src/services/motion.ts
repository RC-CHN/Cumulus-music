export type MotionPreference = 'system' | 'disabled';
export const motionKey = 'cumulus.motion.v1';
export function motionPreference(value: unknown): MotionPreference {
  return value === 'disabled' ? 'disabled' : 'system';
}
export function reducedMotion(preference: MotionPreference, systemReduced: boolean) {
  return preference === 'disabled' || systemReduced;
}
