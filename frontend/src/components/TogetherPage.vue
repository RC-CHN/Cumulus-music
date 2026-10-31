<script setup lang="ts">
import { computed, ref } from 'vue';
import type { TogetherModel } from '../composables/useTogether';
import { time } from '../services/player';
import Icon from './Icon.vue';
const props = defineProps<{ model: TogetherModel }>();
const { view, loading, busy, error, closeError, notice } = props.model;
const scroll = ref(0),
  leave = ref<string>();
const failedAvatars = ref(new Set<string>());
const start = computed(() => Math.max(0, Math.floor(scroll.value / 58) - 3));
const end = computed(() => Math.min(view.value?.tracks.length ?? 0, start.value + 18));
const rows = computed(() => view.value?.tracks.slice(start.value, end.value) ?? []);
const labels: Record<string, string> = {
  create: '创建房间',
  accept: '加入房间',
  end: '退出房间',
  queue: '更新队列',
  play: '同步播放',
};
const unavailable = computed(
  () => busy.value || !!view.value?.pending.length || !!view.value?.ending,
);
const exitPending = computed(() => !!view.value?.pending.some((op) => op.action === 'end'));
const ending = computed(() => !!view.value?.ending || exitPending.value);
const leaveChanged = computed(() => !!leave.value && leave.value !== view.value?.room?.id);
const access = computed(
  () => new Map(view.value?.commonAccess.map((item) => [item.trackId, item]) ?? []),
);
const sharedBusy = computed(() =>
  ['pending', 'failed', 'superseded'].includes(view.value?.sharedPhase ?? ''),
);
function accessLabel(id: string) {
  const value = access.value.get(id);
  if (!value) return '';
  if (value.preview) return `共同试听 ${time(value.preview.start)}–${time(value.preview.end)}`;
  if (value.canPlay) return '';
  const who = { me: '你', other: '对方', both: '双方', none: '', unknown: '' }[value.limitedMember];
  if (value.reason === 'vip') return `${who}需会员权益`;
  if (value.reason === 'digital_album' || value.reason === 'charge_song') return `${who}尚未购买`;
  return '暂不可共同播放';
}
const connectionLabels = {
  unavailable: '消息连接未启用',
  idle: '消息连接待开启',
  connecting: '正在连接消息服务…',
  entering: '正在进入房间…',
  connected: '消息连接已建立',
  reconnecting: '正在恢复消息连接…',
  failed: '消息连接失败',
  closing: '正在关闭消息连接…',
  closed: '消息连接已关闭',
};
async function exit() {
  const id = leave.value;
  if (id !== view.value?.room?.id || ending.value) return;
  if (id) {
    await props.model.command({ kind: 'end', room_id: id });
    if (!view.value?.room) leave.value = undefined;
  }
}
</script>
<template>
  <section class="together-page" aria-label="一起听">
    <header class="together-heading">
      <div>
        <h1>一起听</h1>
        <p>让同一首歌，连接此刻的心情</p>
      </div>
      <button
        class="icon-button"
        aria-label="刷新一起听"
        :disabled="busy || loading"
        @click="model.load(true)"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <p v-if="error" class="error-state" role="alert">{{ error }}</p>
    <p v-if="closeError" class="error-state" role="alert">{{ closeError }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <p v-if="view?.lifecycleNotice" class="notice" role="status">{{ view.lifecycleNotice }}</p>
    <p v-if="ending" class="notice" role="status">
      一起听正在结束。若结果未确认，请先核对退出状态。
    </p>
    <p v-if="!view" class="empty-state" role="status">
      {{ loading ? '正在读取房间…' : '暂未读取到房间状态'
      }}<button v-if="!loading" class="text-button" @click="model.load(true)">重试</button>
    </p>
    <template v-else>
      <div class="together-capability" role="note">
        <Icon name="headphones" /><span>已支持房间管理与本机播放控制，好友实时同步尚未开放。</span>
      </div>
      <section class="together-room">
        <div
          v-if="view.room || view.localRecovery"
          class="together-connection"
          :data-state="view.connection.state"
          role="status"
        >
          <span>{{ view.connection.message || connectionLabels[view.connection.state] }}</span>
          <button
            v-if="
              view.room &&
              view.attached &&
              ['connected', 'failed', 'closed'].includes(view.connection.state)
            "
            class="text-button"
            :disabled="unavailable"
            @click="model.command({ kind: 'reconnect', room_id: view.room.id })"
          >
            重新连接
          </button>
        </div>
        <div class="together-members">
          <template v-if="view.room"
            ><div v-for="(member, index) in view.room.members" :key="index" class="together-person">
              <img
                v-if="member.avatarUrl && !failedAvatars.has(member.avatarUrl)"
                :src="member.avatarUrl"
                alt=""
                referrerpolicy="no-referrer"
                @error="failedAvatars.add(member.avatarUrl!)"
              /><span v-else class="together-avatar"><Icon name="user" /></span
              ><strong
                >{{ member.nickname || '听友' }}<small v-if="member.isSelf">我</small></strong
              >
            </div></template
          >
          <div v-else class="together-person">
            <span class="together-avatar"><Icon name="headphones" /></span>
          </div>
          <span class="together-wave" aria-hidden="true"><i /><i /><i /><i /><i /></span>
          <div v-if="!view.room || view.room.members.length === 1" class="together-person">
            <span class="together-avatar empty"><Icon name="user" /></span
            ><strong>期待另一位听友</strong>
          </div>
        </div>
        <h2>
          {{ ending ? '正在结束一起听' : view.room ? '房间已创建' : '一起分享喜欢的音乐' }}
        </h2>
        <p>
          {{
            view.room
              ? view.attached
                ? ending
                  ? '正在结束当前房间，播放同步已停止'
                  : '当前播放队列和播放条已连接房间'
                : '本机尚未连接这个房间的播放控制'
              : '创建房间后，可用底部播放条控制播放，共享当前歌曲队列。'
          }}
        </p>
        <div class="together-actions">
          <button
            v-if="!view.room && !view.localRecovery"
            class="primary"
            :disabled="unavailable || loading"
            @click="model.command({ kind: 'create' })"
          >
            创建房间
          </button>
          <button
            v-if="view.localRecovery"
            class="primary"
            :disabled="busy"
            @click="model.command({ kind: 'restore_local' })"
          >
            恢复播放设置
          </button>
          <template v-if="view.room">
            <button
              class="primary"
              :disabled="unavailable || view.localRecovery"
              @click="
                model.command({ kind: view.attached ? 'publish' : 'attach', room_id: view.room.id })
              "
            >
              {{ view.attached ? '重新同步当前播放' : '继续本机控制' }}
            </button>
            <button class="text-button" :disabled="busy || ending" @click="leave = view.room.id">
              退出房间
            </button>
          </template>
        </div>
      </section>
      <p v-if="view.syncError" class="error-state" role="alert">
        {{ exitPending ? '退出房间的结果尚未确认，请先核对状态。' : view.syncError }}
      </p>
      <section v-if="view.pending.length" class="together-pending" aria-label="待核对的房间操作">
        <h3>操作结果待确认</h3>
        <p>先核对服务端状态，再继续同步。</p>
        <div v-for="operation in view.pending" :key="operation.id">
          <span>{{ labels[operation.action] }}</span
          ><button
            class="text-button"
            :disabled="busy"
            @click="model.command({ kind: 'reconcile', operation_id: operation.id })"
          >
            核对状态
          </button>
        </div>
      </section>
      <div
        v-if="leave && view.room"
        class="together-leave"
        role="alertdialog"
        aria-label="退出一起听确认"
      >
        <p>退出当前房间？本机音乐将继续播放。</p>
        <p v-if="leaveChanged" class="form-warning" role="alert">
          房间已变化，请取消后核对当前房间。
        </p>
        <button class="primary" :disabled="busy || ending || leaveChanged" @click="exit">
          确认退出</button
        ><button class="text-button" :disabled="busy" @click="leave = undefined">继续收听</button>
      </div>
      <section v-if="view.room" class="together-queue">
        <header>
          <h2>
            {{ ending ? '上次同步的共享队列' : '共享队列' }}
            <small>{{ view.tracks.length }} 首</small>
          </h2>
          <span>{{
            view.mode === 'RANDOM'
              ? '随机播放'
              : view.mode === 'SINGLE_LOOP'
                ? '单曲循环'
                : '列表循环'
          }}</span>
        </header>
        <p v-if="!view.tracks.length" class="empty-state">
          还没有共享歌曲，选择普通歌曲开始播放后即可同步。
        </p>
        <div
          v-else
          class="together-tracks"
          @scroll="scroll = ($event.target as HTMLElement).scrollTop"
        >
          <div :style="{ height: `${start * 58}px` }" aria-hidden="true" />
          <div
            v-for="(track, index) in rows"
            :key="track.id"
            class="together-track"
            :class="{ current: track.id === (view.attached ? view.nativeTargetId : view.targetId) }"
          >
            <span class="together-number">{{ start + index + 1 }}</span
            ><img
              v-if="track.coverUrl"
              :src="track.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            /><span v-else class="together-cover"><Icon name="music" /></span>
            <button
              class="together-track-title"
              :disabled="
                unavailable ||
                !view.attached ||
                sharedBusy ||
                access.get(track.id)?.canPlay === false
              "
              @click="model.command({ kind: 'select', room_id: view.room.id, song_id: track.id })"
            >
              <strong class="ellipsis">{{ track.name }}</strong
              ><small class="ellipsis">{{
                accessLabel(track.id) || track.artists.join(' / ')
              }}</small></button
            ><time>{{ time(track.durationMs / 1000) }}</time
            ><button
              class="icon-button"
              :disabled="unavailable || !view.attached"
              aria-label="从共享队列移除"
              @click="model.command({ kind: 'remove', room_id: view.room.id, song_id: track.id })"
            >
              <Icon name="close" />
            </button>
          </div>
          <div :style="{ height: `${(view.tracks.length - end) * 58}px` }" aria-hidden="true" />
        </div>
      </section>
    </template>
  </section>
</template>
