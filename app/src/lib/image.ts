// Decoding of user images into RGBA pixels, using the browser's own decoders.

/** Images above this are downscaled before vectorization (same order as vectorizer.ai). */
export const MAX_PIXELS = 4_000_000;

export const ACCEPTED_TYPES = [
  'image/png',
  'image/jpeg',
  'image/webp',
  'image/gif',
  'image/bmp',
  'image/avif',
  'image/svg+xml',
];

export type LoadedImage = {
  name: string;
  pixels: ImageData;
  /** Object URL of the (possibly downscaled) raster, for display. */
  url: string;
  originalWidth: number;
  originalHeight: number;
};

/** Pure helper: target size keeping aspect ratio under `maxPixels`. */
export function fitWithin(width: number, height: number, maxPixels = MAX_PIXELS): [number, number] {
  const pixels = width * height;
  if (pixels <= maxPixels) return [width, height];
  const k = Math.sqrt(maxPixels / pixels);
  return [Math.max(1, Math.floor(width * k)), Math.max(1, Math.floor(height * k))];
}

/** createImageBitmap does not accept SVG in every browser: fall back to <img>. */
async function decode(blob: Blob): Promise<ImageBitmap> {
  try {
    return await createImageBitmap(blob);
  } catch {
    const url = URL.createObjectURL(blob);
    try {
      const img = new Image();
      img.src = url;
      await img.decode();
      return await createImageBitmap(img);
    } finally {
      URL.revokeObjectURL(url);
    }
  }
}

export async function loadImage(blob: Blob, name: string): Promise<LoadedImage> {
  const bitmap = await decode(blob);
  const [w, h] = fitWithin(bitmap.width, bitmap.height);
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Canvas 2D indisponible');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(bitmap, 0, 0, w, h);
  const pixels = ctx.getImageData(0, 0, w, h);
  const png = await canvas.convertToBlob({ type: 'image/png' });
  const result: LoadedImage = {
    name,
    pixels,
    url: URL.createObjectURL(png),
    originalWidth: bitmap.width,
    originalHeight: bitmap.height,
  };
  bitmap.close();
  return result;
}

export function baseName(fileName: string): string {
  const i = fileName.lastIndexOf('.');
  return i > 0 ? fileName.slice(0, i) : fileName;
}
