// Pages made over in the browser: the plain margins round a scan cut away, a spread cut in
// two. The pictures come from Mango itself, so a canvas may read them; what comes out is a
// blob address of its own, kept for a few pages and then let go.

export type Part = 'whole' | 'first' | 'second';

type Box = { x: number; y: number; w: number; h: number };

/** Past this many pixels a phone's browser refuses a canvas, or runs out of memory: such a page is left as it is. */
const MOST_PIXELS = 16_000_000;
/** How many made-over pages are kept at once. */
const KEPT = 16;
/** The size a page is looked at in to find its margins. */
const LOOK = 400;

const made = new Map<string, Promise<string | null>>();

/**
 * A page as wanted: inside its margins, one half of it, or both. Null when it is to be shown as it is.
 * The first half is the one read first: the right one in a right-to-left book.
 */
export function makeOver(src: string, crop: boolean, part: Part, rtl: boolean): Promise<string | null> {
  if (!crop && part === 'whole') return Promise.resolve(null);
  const key = `${src}|${crop}|${part}|${rtl}`;
  let found = made.get(key);
  if (!found) {
    found = draw(src, crop, part, rtl).catch(() => null);
    made.set(key, found);
    while (made.size > KEPT) {
      const [oldest, url] = made.entries().next().value!;
      made.delete(oldest);
      url.then((gone) => gone && URL.revokeObjectURL(gone));
    }
  }
  return found;
}

async function draw(src: string, crop: boolean, part: Part, rtl: boolean): Promise<string | null> {
  const image = new Image();
  image.src = src;
  await image.decode();
  const { naturalWidth: width, naturalHeight: height } = image;
  if (!width || !height || width * height > MOST_PIXELS) return null;
  let box: Box = (crop && margins(image)) || { x: 0, y: 0, w: width, h: height };
  if (part !== 'whole') {
    const half = Math.floor(box.w / 2);
    const left = (part === 'first') !== rtl;
    box = { x: left ? box.x : box.x + half, y: box.y, w: left ? half : box.w - half, h: box.h };
  }
  if (box.w === width && box.h === height) return null;
  const canvas = document.createElement('canvas');
  canvas.width = box.w;
  canvas.height = box.h;
  canvas.getContext('2d')!.drawImage(image, box.x, box.y, box.w, box.h, 0, 0, box.w, box.h);
  // A browser that cannot write WebP writes PNG instead, which is lossless anyway.
  const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/webp', 0.95));
  return blob ? URL.createObjectURL(blob) : null;
}

/**
 * The part of a page inside its plain margins, in the page's own pixels. Null when there
 * is none worth cutting, or when the corners do not agree on a plain white or black.
 */
function margins(image: HTMLImageElement): Box | null {
  const scale = Math.min(1, LOOK / Math.max(image.naturalWidth, image.naturalHeight));
  const w = Math.max(1, Math.round(image.naturalWidth * scale));
  const h = Math.max(1, Math.round(image.naturalHeight * scale));
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const context = canvas.getContext('2d', { willReadFrequently: true })!;
  context.drawImage(image, 0, 0, w, h);
  const data = context.getImageData(0, 0, w, h).data;
  const light = (x: number, y: number) => {
    const at = (y * w + x) * 4;
    return (data[at] * 299 + data[at + 1] * 587 + data[at + 2] * 114) / 1000;
  };
  // The margin's colour is the corners', when they agree and it is paper white or frame black.
  const corners = [light(0, 0), light(w - 1, 0), light(0, h - 1), light(w - 1, h - 1)];
  const ground = corners.reduce((sum, value) => sum + value, 0) / corners.length;
  if (corners.some((value) => Math.abs(value - ground) > 30) || (ground > 60 && ground < 200)) return null;
  // A line is plain when nearly all of it is the margin's colour: specks of dust do not count.
  const plain = (x: number, y: number) => Math.abs(light(x, y) - ground) < 40;
  const row = (y: number) => {
    let off = 0;
    for (let x = 0; x < w; x++) if (!plain(x, y)) off++;
    return off <= w * 0.01;
  };
  const column = (x: number, top: number, bottom: number) => {
    let off = 0;
    for (let y = top; y <= bottom; y++) if (!plain(x, y)) off++;
    return off <= (bottom - top + 1) * 0.01;
  };
  let top = 0;
  while (top < h - 1 && row(top)) top++;
  let bottom = h - 1;
  while (bottom > top && row(bottom)) bottom--;
  let left = 0;
  while (left < w - 1 && column(left, top, bottom)) left++;
  let right = w - 1;
  while (right > left && column(right, top, bottom)) right--;
  // A little of the margin stays, so the art does not run into the edge of the screen.
  const pad = 2;
  const x = Math.max(0, Math.floor((left - pad) / scale));
  const y = Math.max(0, Math.floor((top - pad) / scale));
  const box = {
    x,
    y,
    w: Math.min(image.naturalWidth, Math.ceil((right + 1 + pad) / scale)) - x,
    h: Math.min(image.naturalHeight, Math.ceil((bottom + 1 + pad) / scale)) - y,
  };
  const area = image.naturalWidth * image.naturalHeight;
  // Nothing worth the trouble, or so much that it must have been a mistake: a page that is mostly blank.
  if (box.w * box.h > area * 0.97 || box.w < image.naturalWidth * 0.4 || box.h < image.naturalHeight * 0.4) return null;
  return box;
}
