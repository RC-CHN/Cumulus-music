import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { Quality } from '../services/player.ts';
import { validateSettingsQuality, type DefaultQualityView } from '../services/settingsQuality.ts';

export interface QualitySettingsVisit {
  epoch: string;
  page: object;
}
interface Ports {
  visit(): QualitySettingsVisit | undefined;
  read(epoch: string, refresh: boolean): Promise<DefaultQualityView>;
  save(epoch: string, revision: string, quality: Quality): Promise<DefaultQualityView | void>;
  expired(): void;
  changed?(): void;
}

const needsAccount = (code: string) => code === 'session_expired' || code === 'profile_changed';

export function useDefaultQualitySettings(ports: Ports) {
  const value = shallowRef<DefaultQualityView>();
  const activity = ref<'reading' | 'saving'>();
  const feedback = ref('');
  const readbackNeeded = ref(false);
  let generation = 0;
  function live(visit: QualitySettingsVisit, ticket: number) {
    const current = ports.visit();
    return current?.epoch === visit.epoch && current.page === visit.page && ticket === generation;
  }
  function retire() {
    generation++;
    value.value = undefined;
    feedback.value = '';
    activity.value = undefined;
    readbackNeeded.value = false;
  }
  async function read(visit: QualitySettingsVisit, refresh: boolean) {
    return validateSettingsQuality(await ports.read(visit.epoch, refresh), visit.epoch);
  }
  async function refresh(force = false) {
    const visit = ports.visit();
    if (!visit || activity.value) return false;
    const ticket = ++generation;
    activity.value = 'reading';
    feedback.value = '';
    try {
      const result = await read(visit, force);
      if (!live(visit, ticket)) return false;
      const changed = value.value?.quality !== result.quality;
      value.value = result;
      readbackNeeded.value = false;
      if (force && changed) ports.changed?.();
      return true;
    } catch (cause) {
      if (!live(visit, ticket)) return false;
      const issue = failure(cause);
      if (needsAccount(issue.code)) ports.expired();
      else {
        feedback.value = issue.message;
        readbackNeeded.value = true;
      }
      return false;
    } finally {
      if (live(visit, ticket)) activity.value = undefined;
    }
  }
  async function choose(quality: Quality) {
    const visit = ports.visit();
    const before = value.value;
    if (
      !visit ||
      before?.epoch !== visit.epoch ||
      !before.revision ||
      before.quality === quality ||
      !before.choices.some((row) => row.quality === quality && row.access === 'allowed') ||
      activity.value ||
      readbackNeeded.value
    )
      return;
    const ticket = ++generation;
    activity.value = 'saving';
    feedback.value = '';
    try {
      let response: DefaultQualityView | void = undefined;
      let problem = '';
      try {
        response = await ports.save(visit.epoch, before.revision, quality);
      } catch (cause) {
        if (!live(visit, ticket)) return;
        const issue = failure(cause);
        if (needsAccount(issue.code)) {
          ports.expired();
          return;
        }
        problem = ['snapshot_changed', 'stale', 'playback_quality_settings_changed'].includes(
          issue.code,
        )
          ? '音质或权益已变化，已重新读取当前设置。'
          : issue.code.startsWith('playback_quality_')
            ? issue.message
            : '未收到保存确认，已重新读取当前设置。';
      }
      if (!live(visit, ticket)) return;
      try {
        const result = response
          ? validateSettingsQuality(response, visit.epoch)
          : await read(visit, false);
        if (!live(visit, ticket)) return;
        value.value = result;
        feedback.value = problem || (result.quality === quality ? '设置已更新' : '此音质未保存。');
        ports.changed?.();
      } catch (cause) {
        if (!live(visit, ticket)) return;
        if (needsAccount(failure(cause).code)) ports.expired();
        else {
          feedback.value = '无法确认当前音质，请重新读取。';
          readbackNeeded.value = true;
        }
      }
    } finally {
      if (live(visit, ticket)) activity.value = undefined;
    }
  }
  return { value, activity, feedback, readbackNeeded, refresh, choose, retire };
}
