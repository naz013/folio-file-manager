/** What the core says about a picture (`read_image_info`); every field may be missing. */
export interface ImageInfo {
  width: number | null;
  height: number | null;
  camera: string | null;
  lens: string | null;
  /** `YYYY-MM-DD HH:MM:SS`, as the camera wrote it. */
  taken: string | null;
  /** Shutter, aperture, ISO and focal length as one line. */
  exposure: string | null;
  /** The picture says where it was taken; the place is only sent when it was asked for. */
  hasLocation: boolean;
  location: { lat: number; lon: number } | null;
}

/** Pictures the core can read details from. */
const DETAIL_EXTENSIONS = ["jpg", "jpeg", "png", "webp", "heic", "heif", "tif", "tiff"];

/** Whether a file of this name may have details to show. */
export function hasDetails(name: string): boolean {
  const dot = name.lastIndexOf(".");
  return dot > 0 && DETAIL_EXTENSIONS.includes(name.slice(dot + 1).toLowerCase());
}

/** `6000 × 4000 px`, or nothing if the size is not known. */
export function sizeText(info: ImageInfo): string | null {
  return info.width && info.height ? `${info.width} × ${info.height} px` : null;
}

/** The place in the usual way: `33.85000° S, 151.21000° E`. */
export function placeText(place: { lat: number; lon: number }): string {
  const part = (n: number, plus: string, minus: string) =>
    `${Math.abs(n).toFixed(5)}° ${n < 0 ? minus : plus}`;
  return `${part(place.lat, "N", "S")}, ${part(place.lon, "E", "W")}`;
}

/** The facts to show, in order, as label and text; the place is not among them. */
export function facts(info: ImageInfo): { label: string; text: string }[] {
  const rows: [string, string | null][] = [
    ["Size", sizeText(info)],
    ["Camera", info.camera],
    ["Lens", info.lens],
    ["Taken", info.taken],
    ["Exposure", info.exposure],
  ];
  return rows.flatMap(([label, text]) => (text ? [{ label, text }] : []));
}
