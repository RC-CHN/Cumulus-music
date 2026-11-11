import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import {
  isDownloadLayoutSettings,
  isDownloadLayoutValue,
  type DownloadLayoutChange,
  type DownloadLayoutField,
  type DownloadLayoutSettings,
  type DownloadLayoutValue,
} from '../services/downloads.ts';
import type { DownloadDirectorySettingsOwner } from './useDownloadDirectorySettings.ts';

type Owner = DownloadDirectorySettingsOwner;
interface LayoutPorts {
  owner(): Owner | undefined;
  blocked(): boolean;
  read(epoch: string): Promise<DownloadLayoutSettings>;
  change(epoch: string, change: DownloadLayoutChange): Promise<DownloadLayoutSettings>;
  directoryChanged(): void;
  expired(): void;
}

const invalid = { code: 'invalid_layout', message: '读取到的下载命名或分类无效。' };
const needsReadback = '未能核对下载命名与分类，请重新读取。';
const notConfirmed = '更改未得到确认，已显示当前下载设置。';
const accountExpired = (code: string) => code === 'session_expired' || code === 'profile_changed';

export function createDownloadLayoutSettings(ports: LayoutPorts) {
  const value = shallowRef<DownloadLayoutSettings>();
  const pending = shallowRef<{ field: DownloadLayoutField; value: DownloadLayoutValue }>();
  const activity = ref<'reading' | 'saving'>();
  const feedback = shallowRef<{ text: string; recheck?: boolean }>();
  let generation = 0;

  function live(owner: Owner) {
    const current = ports.owner();
    return current?.epoch === owner.epoch && current.visit === owner.visit;
  }
  function shown(field: DownloadLayoutField) {
    if (pending.value?.field === field) return pending.value.value;
    return field === 'file_type' ? value.value?.fileType : value.value?.pathType;
  }
  function retire() {
    generation++;
    value.value = undefined;
    pending.value = undefined;
    activity.value = undefined;
    feedback.value = undefined;
  }
  async function read(owner: Owner) {
    const result = await ports.read(owner.epoch);
    if (!isDownloadLayoutSettings(result)) throw invalid;
    return result;
  }
  function readFailed(cause: unknown) {
    const issue = failure(cause);
    if (accountExpired(issue.code)) {
      ports.expired();
      return false;
    }
    feedback.value = {
      text: issue.code === invalid.code ? issue.message : needsReadback,
      recheck: true,
    };
    return true;
  }
  function canChoose() {
    return !!(
      ports.owner() &&
      value.value &&
      !activity.value &&
      !feedback.value?.recheck &&
      !ports.blocked()
    );
  }
  async function refresh(owner = ports.owner()) {
    if (!owner || !live(owner) || activity.value === 'saving') return;
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    activity.value = 'reading';
    try {
      const result = await read(owner);
      if (!current()) return;
      value.value = result;
      feedback.value = undefined;
    } catch (cause) {
      if (current()) readFailed(cause);
    } finally {
      if (generation === ticket) activity.value = undefined;
    }
  }
  async function choose(field: DownloadLayoutField, option: DownloadLayoutValue) {
    const owner = ports.owner();
    const before = value.value;
    if (
      !owner ||
      !before ||
      !canChoose() ||
      !isDownloadLayoutValue(option) ||
      shown(field) === option
    )
      return;
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    let reloadDirectory = true;
    let problem: string | undefined;
    feedback.value = undefined;
    activity.value = 'saving';
    pending.value = { field, value: option };
    try {
      try {
        const result = await ports.change(owner.epoch, {
          field,
          value: option,
          expectedRevision: before.revision,
        });
        if (!current()) return;
        if (!isDownloadLayoutSettings(result)) problem = notConfirmed;
      } catch (cause) {
        if (!current()) return;
        const issue = failure(cause);
        if (accountExpired(issue.code)) {
          reloadDirectory = false;
          ports.expired();
          return;
        }
        problem =
          issue.code === 'stale'
            ? '下载命名或分类已变化，已显示当前设置。'
            : issue.code === 'connection'
              ? notConfirmed
              : issue.message;
      }
      try {
        const result = await read(owner);
        if (!current()) return;
        value.value = result;
        const observed = field === 'file_type' ? result.fileType : result.pathType;
        if (problem || observed !== option) feedback.value = { text: problem ?? notConfirmed };
      } catch (cause) {
        if (current()) reloadDirectory = readFailed(cause);
      }
    } finally {
      if (generation === ticket) {
        pending.value = undefined;
        activity.value = undefined;
        if (current() && reloadDirectory) ports.directoryChanged();
      }
    }
  }
  return { value, activity, feedback, shown, canChoose, choose, refresh, retire };
}
