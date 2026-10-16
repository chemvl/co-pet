import { describe, expect, it } from 'vitest';
import { crc32, pngWithDpi } from './dragImage';

function chunk(type: string, data: number[]): number[] {
  const body = Uint8Array.from([...type].map((c) => c.charCodeAt(0)).concat(data));
  const len = data.length;
  const crc = crc32(body);
  return [len >>> 24, (len >>> 16) & 255, (len >>> 8) & 255, len & 255, ...body, crc >>> 24, (crc >>> 16) & 255, (crc >>> 8) & 255, crc & 255];
}

const SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];
const IHDR = chunk('IHDR', [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
const IEND = chunk('IEND', []);

/** Chunk types in order, with each chunk's CRC checked. */
function chunks(png: Uint8Array): { type: string; data: Uint8Array }[] {
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
  const out = [];
  for (let at = 8; at < png.length; ) {
    const len = view.getUint32(at);
    const typeAndData = png.subarray(at + 4, at + 8 + len);
    expect(view.getUint32(at + 8 + len)).toBe(crc32(typeAndData));
    out.push({ type: String.fromCharCode(...typeAndData.subarray(0, 4)), data: typeAndData.subarray(4) });
    at += 12 + len;
  }
  return out;
}

describe('pngWithDpi', () => {
  it('computes the standard CRC-32', () => {
    // The CRC-32 check value from the PNG / zlib specification.
    expect(crc32(new TextEncoder().encode('123456789'))).toBe(0xcbf43926);
  });

  it('inserts a valid 144 dpi pHYs chunk right after IHDR', () => {
    const png = pngWithDpi(Uint8Array.from([...SIGNATURE, ...IHDR, ...IEND]), 144);
    const list = chunks(png);
    expect(list.map((c) => c.type)).toEqual(['IHDR', 'pHYs', 'IEND']);
    const phys = new DataView(list[1].data.buffer, list[1].data.byteOffset, 9);
    expect(phys.getUint32(0)).toBe(5669);
    expect(phys.getUint32(4)).toBe(5669);
    expect(phys.getUint8(8)).toBe(1);
  });

  it('replaces an existing pHYs chunk', () => {
    const old = chunk('pHYs', [0, 0, 11, 19, 0, 0, 11, 19, 1]);
    const png = pngWithDpi(Uint8Array.from([...SIGNATURE, ...IHDR, ...old, ...IEND]), 144);
    expect(chunks(png).map((c) => c.type)).toEqual(['IHDR', 'pHYs', 'IEND']);
  });
});
