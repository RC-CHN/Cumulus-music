// M/54 -> M/279 -> M/258: browser decode and default canvas sampling only.
// The native host fetches and validates the cover; this module never accepts a URL.
export const SCENE_COVER_MAX_BYTES = 3 * 1024 * 1024;
export const SCENE_COVER_MAX_DECODE_PIXELS = 12_000_000;
export const SCENE_COVER_MAX_SAMPLE_PIXELS = 16_384;
export interface SceneCoverImage {
  mimeType: string;
  bytes: number[];
  width: number;
  height: number;
}
export interface SceneCoverPixels {
  width: number;
  height: number;
  rgba: number[];
}
export interface SceneCanvasEnvironment {
  image(): HTMLImageElement;
  canvas(): HTMLCanvasElement;
  objectUrl(bytes: Uint8Array<ArrayBuffer>, mimeType: string): string;
  revoke(url: string): void;
}
const browser: SceneCanvasEnvironment = {
  image: () => new Image(),
  canvas: () => document.createElement('canvas'),
  objectUrl: (bytes, mimeType) => URL.createObjectURL(new Blob([bytes], { type: mimeType })),
  revoke: (url) => URL.revokeObjectURL(url),
};
export function sceneSampleDimensions(width: number, height: number) {
  if (
    !Number.isSafeInteger(width) ||
    !Number.isSafeInteger(height) ||
    width <= 0 ||
    height <= 0 ||
    width > 4096 ||
    height > 4096 ||
    width * height > SCENE_COVER_MAX_DECODE_PIXELS
  )
    throw Error('Scene cover dimensions exceed the local decode limit');
  const sampledWidth = Math.min(width, 50);
  const sampledHeight = Math.ceil((sampledWidth * height) / width);
  if (sampledWidth * sampledHeight > SCENE_COVER_MAX_SAMPLE_PIXELS)
    throw Error('Scene cover aspect exceeds the local canvas limit');
  return { width: sampledWidth, height: sampledHeight };
}

export function sampleSceneCover(
  source: SceneCoverImage,
  signal: AbortSignal,
  environment: SceneCanvasEnvironment = browser,
): Promise<SceneCoverPixels> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(Error('Scene cover sampling was cancelled'));
      return;
    }
    if (
      !/^image\/(png|jpeg|webp|gif|bmp)$/.test(source.mimeType) ||
      !Array.isArray(source.bytes) ||
      !source.bytes.length ||
      source.bytes.length > SCENE_COVER_MAX_BYTES ||
      !source.bytes.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
    ) {
      reject(Error('Invalid bounded Scene cover image'));
      return;
    }
    const image = environment.image();
    let url: string;
    try {
      url = environment.objectUrl(new Uint8Array(source.bytes), source.mimeType);
    } catch (cause) {
      reject(cause);
      return;
    }
    let done = false;
    const finish = (pixels?: SceneCoverPixels, cause?: unknown) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      signal.removeEventListener('abort', cancelled);
      image.onload = image.onerror = null;
      image.removeAttribute('src');
      environment.revoke(url);
      if (pixels) resolve(pixels);
      else reject(cause);
    };
    const cancelled = () => finish(undefined, Error('Scene cover sampling was cancelled'));
    const timer = setTimeout(
      () => finish(undefined, Error('Scene cover decode timed out')),
      10_000,
    );
    signal.addEventListener('abort', cancelled, { once: true });
    image.onload = () => {
      if (signal.aborted) {
        cancelled();
        return;
      }
      try {
        image.width = image.naturalWidth;
        image.height = image.naturalHeight;
        if (image.naturalWidth !== source.width || image.naturalHeight !== source.height)
          throw Error('Scene cover decode does not match the native image receipt');
        const { width, height } = sceneSampleDimensions(image.naturalWidth, image.naturalHeight);
        const canvas = environment.canvas();
        canvas.width = width;
        canvas.height = height;
        const context = canvas.getContext('2d');
        if (!context) throw Error('Scene cover canvas is unavailable');
        // Leave the browser's smoothing defaults intact, matching M/258.
        context.drawImage(image, 0, 0, width, height);
        const rgba = context.getImageData(0, 0, width, height).data;
        if (rgba.length !== width * height * 4)
          throw Error('Scene cover canvas returned incomplete pixels');
        finish({ width, height, rgba: Array.from(rgba) });
      } catch (cause) {
        finish(undefined, cause);
      }
    };
    image.onerror = () => finish(undefined, Error('Scene cover could not be decoded'));
    image.src = url;
  });
}
