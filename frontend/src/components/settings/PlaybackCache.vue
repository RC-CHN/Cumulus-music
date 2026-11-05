<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { usePlaybackCache } from '../../composables/usePlaybackCache';
import { useControlFocus } from '../../composables/useControlFocus';
import { playbackCacheQuota, playbackCacheUsage } from '../../services/playbackCache';

const model = usePlaybackCache();
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => 'playback-cache',
  () => refresh.value,
);

function blockPending(event: PointerEvent | KeyboardEvent) {
  if (!model.canChange.value && (!('key' in event) || !['Tab', 'Escape'].includes(event.key)))
    event.preventDefault();
}

function preview(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  model.preview(Number(input.value));
  if (!model.canChange.value && model.capacity.value !== undefined)
    input.value = String(model.capacity.value);
}

function commit(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const value = Number(input.value);
  void focus.run(event, async () => {
    await model.setCapacity(value);
    if (input.isConnected && model.capacity.value !== undefined)
      input.value = String(model.capacity.value);
  });
}

onMounted(() => void model.read());
onUnmounted(model.dispose);
</script>

<template>
  <section
    class="settings-section playback-cache"
    aria-labelledby="settings-playback-cache"
    :aria-busy="!!model.pending.value"
    data-playback-cache-settings
  >
    <h2 id="settings-playback-cache">在线播放缓存</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>本机缓存</span>
        <button
          ref="refresh"
          type="button"
          class="text-button"
          aria-label="刷新播放缓存"
          :disabled="!!model.pending.value"
          @click="focus.run($event, model.read)"
        >
          刷新
        </button>
      </div>
      <template v-if="model.view.value">
        <dl class="playback-cache-usage" :data-stale="model.stale.value || undefined">
          <dt>缓存目录</dt>
          <dd data-playback-cache-directory>{{ model.view.value.directory }}</dd>
          <dt>已写入缓存</dt>
          <dd data-playback-cache-occupied :data-bytes="model.view.value.occupiedBytes">
            {{ playbackCacheUsage(model.view.value.occupiedBytes) }}
          </dd>
          <dt>已占用额度</dt>
          <dd data-playback-cache-reserved :data-bytes="model.view.value.reservedBytes">
            {{ playbackCacheUsage(model.view.value.reservedBytes) }}
          </dd>
          <dt>可立即释放</dt>
          <dd data-playback-cache-reclaimable :data-bytes="model.view.value.reclaimableBytes">
            {{ playbackCacheUsage(model.view.value.reclaimableBytes) }}
          </dd>
        </dl>
        <div class="playback-cache-capacity">
          <label for="playback-cache-capacity">缓存最大占用</label>
          <input
            id="playback-cache-capacity"
            type="range"
            :min="model.view.value.minCapacityBytes"
            :max="model.view.value.maxCapacityBytes"
            :step="model.view.value.capacityStepBytes"
            :value="model.capacity.value"
            :aria-valuetext="
              playbackCacheQuota(model.capacity.value ?? model.view.value.capacityBytes, true)
            "
            :aria-disabled="!model.canChange.value || undefined"
            :disabled="!model.canChange.value && !model.pending.value"
            data-playback-cache-capacity
            @pointerdown="blockPending"
            @keydown="blockPending"
            @input="preview"
            @change="commit"
          />
          <span aria-hidden="true">
            {{ playbackCacheQuota(model.capacity.value ?? model.view.value.capacityBytes) }}
          </span>
          <button
            type="button"
            class="secondary"
            :disabled="!model.canClear.value"
            data-playback-cache-clear
            @click="focus.run($event, model.clear)"
          >
            清除缓存
          </button>
        </div>
        <p v-if="model.view.value.overCapacity" class="settings-note" role="status">
          缓存占用额度暂时超过上限，等正在使用的数据释放后会继续缓存。
        </p>
        <p class="settings-note">
          此目录由 Cumulus
          管理，只存放部分在线歌曲的临时播放缓存。清理时，正在使用的数据会在使用结束后释放。
        </p>
      </template>
      <p v-else-if="model.view.value === null" class="settings-note" role="status">
        当前设备的在线播放缓存不可用，歌曲仍按原方式播放。
      </p>
      <p v-if="model.pending.value" class="settings-note" role="status" data-playback-cache-status>
        {{
          model.pending.value === 'read'
            ? '正在读取播放缓存…'
            : model.pending.value === 'capacity'
              ? '正在保存缓存容量…'
              : '正在清除缓存…'
        }}
      </p>
      <p
        v-else-if="model.result.value"
        class="settings-note"
        role="status"
        data-playback-cache-status
      >
        {{
          model.result.value === 'capacity'
            ? '设置已更新'
            : model.view.value?.reservedBytes
              ? '可清理的缓存已清除。仍在使用的数据会稍后释放。'
              : '缓存已清除！'
        }}
      </p>
      <p v-if="model.issue.value" class="catalog-error" role="alert" data-playback-cache-error>
        {{ model.issue.value }}
      </p>
    </div>
  </section>
</template>

<style scoped>
.playback-cache-usage {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 8px 18px;
  margin: 12px 0;
  font-size: 13px;
}
.playback-cache-usage dt {
  color: var(--muted);
}
.playback-cache-usage dd {
  margin: 0;
  overflow-wrap: anywhere;
}
.playback-cache-usage[data-stale] {
  color: var(--muted);
}
.playback-cache-capacity {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px 12px;
  margin: 14px 0 8px;
}
.playback-cache-capacity input {
  width: clamp(100px, 32vw, 320px);
  max-width: 100%;
  accent-color: var(--accent);
}
.playback-cache-capacity input[aria-disabled='true'] {
  opacity: 0.65;
}
</style>
