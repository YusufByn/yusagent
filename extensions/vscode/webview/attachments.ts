// Images attached to a message: read in the panel, shrunk when the
// `images.autoResize` setting asks for it (the agent has nothing to shrink
// them with), with a small copy to show.
import type { Attachment } from "../src/protocol";

/** What the providers read. */
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
/** The agent's limit on an image's base64 payload under `images.autoResize`. */
const MAX_BASE64 = 4_500_000;
/** The longest side a shrunk image keeps. */
const MAX_SIDE = 2000;
const THUMB_SIDE = 160;

/** `resize`: shrink an image past the limits. Rejects with a sentence to show. */
export async function attach(file: File, resize: boolean): Promise<Attachment> {
  if (!IMAGE_TYPES.includes(file.type)) {
    throw new Error(`${file.name || "This file"} is not a PNG, JPEG, GIF or WebP image.`);
  }
  const url = await readDataUrl(file);
  const image = await load(url);
  const name = file.name || "pasted image";
  const thumb = draw(image, THUMB_SIDE, "image/jpeg", 0.8);
  const original = { data: base64(url), mimeType: file.type };
  const fits = original.data.length <= MAX_BASE64 && Math.max(image.naturalWidth, image.naturalHeight) <= MAX_SIDE;
  if (!resize || fits) return { ...original, thumb, name };
  return { ...shrink(image, file.type), thumb, name };
}

/**
 * Within the longest side, as PNG when it was one and that fits, else as
 * JPEG at falling quality, then smaller, until the payload fits.
 */
function shrink(image: HTMLImageElement, type: string): { data: string; mimeType: string } {
  for (let side = MAX_SIDE; side >= 256; side = Math.round(side * 0.75)) {
    if (type === "image/png") {
      const data = base64(draw(image, side, "image/png"));
      if (data.length <= MAX_BASE64) return { data, mimeType: "image/png" };
    }
    for (const quality of [0.85, 0.7, 0.55, 0.4]) {
      const data = base64(draw(image, side, "image/jpeg", quality));
      if (data.length <= MAX_BASE64) return { data, mimeType: "image/jpeg" };
    }
  }
  throw new Error("This image stays too large once shrunk.");
}

/** The image within `side` pixels on its longest side, as a data URL. */
function draw(image: HTMLImageElement, side: number, type: string, quality?: number): string {
  const scale = Math.min(1, side / Math.max(image.naturalWidth, image.naturalHeight));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
  const context = canvas.getContext("2d");
  if (!context) throw new Error("This image could not be read.");
  // JPEG has no transparency: a light ground rather than black.
  if (type === "image/jpeg") {
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, canvas.width, canvas.height);
  }
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL(type, quality);
}

function readDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error(`${file.name || "This image"} could not be read.`));
    reader.readAsDataURL(file);
  });
}

function load(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("This image could not be read."));
    image.src = url;
  });
}

function base64(dataUrl: string): string {
  return dataUrl.slice(dataUrl.indexOf(",") + 1);
}
