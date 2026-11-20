/** The view transform of the atlas: world to screen is `screen = world * k + (x, y)`. Pure helpers; the view owns the state. */
export type Camera = { x: number; y: number; k: number };

export const MIN_ZOOM = 0.04;
export const MAX_ZOOM = 12;

export const clampZoom = (k: number): number => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));
export const toWorld = (camera: Camera, sx: number, sy: number) => ({ x: (sx - camera.x) / camera.k, y: (sy - camera.y) / camera.k });
export const toScreen = (camera: Camera, wx: number, wy: number) => ({ x: wx * camera.k + camera.x, y: wy * camera.k + camera.y });

/** Zooms by `factor` keeping the world point under (sx, sy) where it is. */
export function zoomAt(camera: Camera, sx: number, sy: number, factor: number): Camera {
  const k = clampZoom(camera.k * factor);
  const world = toWorld(camera, sx, sy);
  return { k, x: sx - world.x * k, y: sy - world.y * k };
}

export function fitCamera(bounds: { minX: number; minY: number; maxX: number; maxY: number } | null, width: number, height: number, padding = 56): Camera {
  if (!bounds || width <= 0 || height <= 0) return { x: width / 2, y: height / 2, k: 1 };
  const w = Math.max(1, bounds.maxX - bounds.minX);
  const h = Math.max(1, bounds.maxY - bounds.minY);
  const k = clampZoom(Math.min((width - padding * 2) / w, (height - padding * 2) / h, 4));
  return { k, x: width / 2 - ((bounds.minX + bounds.maxX) / 2) * k, y: height / 2 - ((bounds.minY + bounds.maxY) / 2) * k };
}

/** Centres a world point at zoom `k` (search results, "open this group"). */
export function centerOn(wx: number, wy: number, k: number, width: number, height: number): Camera {
  return { k: clampZoom(k), x: width / 2 - wx * clampZoom(k), y: height / 2 - wy * clampZoom(k) };
}

/** Eased interpolation between two cameras, zooming in log space so the speed feels even. */
export function mixCamera(a: Camera, b: Camera, t: number): Camera {
  const e = 1 - (1 - t) ** 3;
  const k = Math.exp(Math.log(a.k) + (Math.log(b.k) - Math.log(a.k)) * e);
  return { k, x: a.x + (b.x - a.x) * e, y: a.y + (b.y - a.y) * e };
}
