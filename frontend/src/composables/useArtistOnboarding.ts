import { ref, shallowRef, watch } from 'vue';
import type { AccountView, TrackView } from '../services/contracts';
import type {
  ArtistCandidate,
  ArtistOffer,
  ArtistOnboardingClient,
  ArtistSave,
  ArtistResultAction,
} from '../services/artistOnboarding';
import type { Quality } from '../services/player';

type Step = 'select' | 'saving' | 'preview' | 'result';
export function useArtistOnboarding(
  core: ArtistOnboardingClient,
  account: () => AccountView,
  enabled: () => boolean,
) {
  const offer = shallowRef<ArtistOffer>();
  const candidates = shallowRef<ArtistCandidate[]>([]);
  const selected = shallowRef<ArtistCandidate[]>([]);
  const songs = shallowRef<TrackView[]>([]);
  const step = ref<Step>('select');
  const saved = ref<ArtistSave>();
  const busy = ref(false);
  const error = ref('');
  let epoch = '';
  let visit = 0;
  let checked = false;
  const similarStarted = new Set<string>();
  function active(request: number, owner = epoch) {
    return visit === request && owner === account().epoch && account().status === 'authenticated';
  }
  function reset() {
    visit++;
    checked = false;
    offer.value = undefined;
    candidates.value = [];
    selected.value = [];
    songs.value = [];
    saved.value = undefined;
    error.value = '';
    busy.value = false;
    step.value = 'select';
    similarStarted.clear();
  }
  watch(
    () => [account().epoch, account().status, enabled()] as const,
    async ([owner, status, visible]) => {
      if (owner !== epoch || status !== 'authenticated') {
        reset();
        epoch = owner;
      }
      if (!visible || status !== 'authenticated' || checked) return;
      const request = visit;
      checked = true;
      try {
        const reply = await core.artistOffer(owner);
        if (!active(request, owner) || reply.epoch !== owner || !reply.value) return;
        offer.value = reply.value;
        await read();
      } catch {
        // An unverified or unavailable offer never interrupts login.
      }
    },
    { immediate: true },
  );
  async function read() {
    const item = offer.value;
    if (!item || busy.value || step.value !== 'select') return;
    const request = visit;
    busy.value = true;
    error.value = '';
    try {
      const reply = await core.artistCandidates(epoch, item.id);
      if (active(request) && reply.epoch === epoch) candidates.value = reply.value;
    } catch {
      if (active(request)) error.value = '推荐歌手暂不可用';
    } finally {
      if (active(request)) busy.value = false;
    }
  }
  async function toggle(artist: ArtistCandidate) {
    const item = offer.value;
    if (!item || busy.value || item.saveBlocked || step.value !== 'select') return;
    const found = selected.value.some((candidate) => candidate.id === artist.id);
    if (found) {
      selected.value = selected.value.filter((candidate) => candidate.id !== artist.id);
      return;
    }
    if (selected.value.length >= 50) return;
    selected.value = [...selected.value, artist];
    if (similarStarted.has(artist.id)) return;
    similarStarted.add(artist.id);
    const request = visit;
    try {
      const reply = await core.artistCandidates(epoch, item.id, artist.id);
      if (!active(request) || reply.epoch !== epoch || step.value !== 'select') return;
      const rows = candidates.value;
      const parent = rows.findIndex((candidate) => candidate.id === artist.id);
      const existing = new Set(rows.map((candidate) => candidate.id));
      const next = reply.value.filter((candidate) => !existing.has(candidate.id));
      if (parent >= 0 && next.length)
        candidates.value = [...rows.slice(0, parent + 1), ...next, ...rows.slice(parent + 1)];
    } catch {
      // Similar artists are optional; the original choice remains selected.
    }
  }
  async function preview() {
    const item = offer.value;
    if (!item || busy.value || !saved.value) return;
    const request = visit;
    step.value = 'preview';
    busy.value = true;
    error.value = '';
    try {
      const reply = await core.artistPreview(epoch, item.id);
      if (!active(request) || reply.epoch !== epoch) return;
      songs.value = reply.value;
      if (songs.value.length) step.value = 'result';
      else error.value = '没有返回推荐歌曲';
    } catch {
      if (active(request)) error.value = '暂时无法读取推荐歌曲';
    } finally {
      if (active(request)) busy.value = false;
    }
  }
  async function save() {
    const item = offer.value;
    if (
      !item ||
      busy.value ||
      item.saveBlocked ||
      step.value !== 'select' ||
      selected.value.length < 2
    )
      return;
    const request = visit;
    const ids = selected.value.map((artist) => artist.id);
    step.value = 'saving';
    busy.value = true;
    error.value = '';
    try {
      const reply = await core.artistSave(epoch, item.id, ids);
      if (!active(request) || reply.epoch !== epoch) return;
      saved.value = reply.value;
    } catch {
      if (!active(request)) return;
      // The renderer cannot know whether the save reached native/server.
      // Any continuation reads the existing native receipt; never replay it.
      saved.value = 'uncertain';
    } finally {
      if (active(request)) busy.value = false;
    }
    if (active(request)) await preview();
  }
  async function close() {
    const item = offer.value;
    if (!item || busy.value) return false;
    const request = visit;
    busy.value = true;
    try {
      await core.artistClose(epoch, item.id);
      if (!active(request)) return false;
      reset();
      checked = true;
      return true;
    } catch {
      if (active(request)) error.value = '暂时无法结束引导，请重试';
      return false;
    } finally {
      if (active(request)) busy.value = false;
    }
  }
  async function result(action: ArtistResultAction, revision: string, quality: Quality) {
    const item = offer.value;
    if (!item || step.value !== 'result' || !songs.value.length || busy.value) return;
    const request = visit;
    busy.value = true;
    error.value = '';
    try {
      await core.artistResult(epoch, item.id, action, revision, quality);
    } catch {
      if (active(request)) error.value = '当前无法操作播放列表，请重试';
      return;
    } finally {
      if (active(request)) busy.value = false;
    }
    if (active(request)) await close();
  }
  return {
    offer,
    candidates,
    selected,
    songs,
    step,
    saved,
    busy,
    error,
    read,
    toggle,
    save,
    preview,
    close,
    result,
  };
}
