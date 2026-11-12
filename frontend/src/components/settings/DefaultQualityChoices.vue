<script setup lang="ts">
import { computed } from 'vue';
import type { Quality } from '../../services/player';
import {
  displayedSettingsQuality,
  settingsQualityAccessLabel,
  settingsQualityLabel,
  type DefaultQualityView,
} from '../../services/settingsQuality';

const props = defineProps<{
  kind: 'playback' | 'download';
  value?: DefaultQualityView;
  activity?: 'reading' | 'saving';
  feedback: string;
  needsReadback: boolean;
  signedIn: boolean;
  available: boolean;
}>();
const emit = defineEmits<{ choose: [quality: Quality]; refresh: []; login: []; member: [] }>();
const displayed = computed(() => displayedSettingsQuality(props.value?.quality));
const blocked = computed(() => !!props.activity || !props.value?.revision || props.needsReadback);
const retained = computed(() => {
  const current = props.value?.quality;
  if (!current) return;
  if (current === 'higher') return '较高（设置中显示为标准）';
  if (props.value?.choices.find((row) => row.quality === current)?.access !== 'allowed')
    return settingsQualityLabel(current);
});
const needsMember = computed(() =>
  props.value?.choices.some(
    (row) => row.access === 'requires_vip' || row.access === 'requires_svip',
  ),
);
function restoreChecked(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  for (const radio of input.closest('fieldset')?.querySelectorAll<HTMLInputElement>('input') ?? [])
    radio.checked = radio.value === displayed.value;
}
function choose(event: Event, quality: Quality) {
  // Do not present an unchecked native write as an accepted preference. This
  // also restores the previous selection on native arrow-key changes.
  restoreChecked(event);
  if (!blocked.value && displayed.value !== quality) emit('choose', quality);
}
function guardKey(event: KeyboardEvent) {
  if (blocked.value && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', ' '].includes(event.key))
    event.preventDefault();
}
</script>

<template>
  <fieldset
    class="settings-options settings-quality-group"
    :data-settings-download-quality="kind === 'download' ? '' : undefined"
    :data-settings-playback-quality="kind === 'playback' ? '' : undefined"
    @keydown="guardKey"
  >
    <legend>{{ kind === 'playback' ? '音质播放设置' : '音质下载设置' }}</legend>
    <div v-if="value" class="settings-quality-choices">
      <label v-for="row in value.choices" :key="row.quality" class="private-checkbox">
        <input
          type="radio"
          :name="`settings-${kind}-quality`"
          :value="row.quality"
          :checked="displayed === row.quality"
          :disabled="row.access !== 'allowed'"
          :aria-disabled="blocked || undefined"
          @click="blocked && $event.preventDefault()"
          @change="choose($event, row.quality)"
        />
        <span>
          <span class="download-quality-name">{{ settingsQualityLabel(row.quality) }}</span>
          <small v-if="settingsQualityAccessLabel[row.access]" class="muted">{{
            settingsQualityAccessLabel[row.access]
          }}</small>
        </span>
      </label>
    </div>
    <p v-if="retained" class="settings-note">保留已保存的音质：{{ retained }}</p>
    <p v-if="value?.damaged" class="settings-note">原生音质设置已损坏，请在播放器中修复。</p>
    <p v-else-if="value && value.revision === null" class="settings-note">
      当前播放音质有试用或未完成的核对，请先在播放器中处理。
    </p>
    <p v-if="activity" class="settings-note" role="status">
      {{ activity === 'saving' ? '正在保存…' : '正在读取音质…' }}
    </p>
    <p
      v-if="feedback"
      class="settings-note"
      :data-settings-download-quality-feedback="kind === 'download' ? '' : undefined"
      :data-settings-playback-quality-feedback="kind === 'playback' ? '' : undefined"
      role="status"
    >
      {{ feedback }}
    </p>
    <div class="settings-quality-actions">
      <button
        v-if="signedIn || (available && needsReadback)"
        type="button"
        class="text-button"
        :aria-disabled="!!activity || undefined"
        @click="!activity && emit('refresh')"
      >
        {{ kind === 'download' ? '刷新下载音质' : '刷新播放音质' }}
      </button>
      <button
        v-if="!signedIn && (!available || value)"
        type="button"
        class="text-button"
        @click="emit('login')"
      >
        {{ kind === 'download' ? '登录后设置下载音质' : '登录查看更多播放音质' }}
      </button>
      <button
        v-if="signedIn && needsMember"
        type="button"
        class="text-button"
        @click="emit('member')"
      >
        查看会员权益
      </button>
    </div>
  </fieldset>
</template>
