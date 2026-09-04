// Generates the source app icon as a PNG, no image library needed.
// The mark is the UI's own motif: stacked recommendation blocks, each with a
// coloured confidence rail — teal for statistics, amber for rules.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 1024;
const px = new Uint8Array(S * S * 4);

const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const fill = (x0, y0, w, h, colour, r = 0) => {
  const [cr, cg, cb] = hex(colour);
  for (let y = y0; y < y0 + h; y++) {
    for (let x = x0; x < x0 + w; x++) {
      if (r > 0) {
        const dx = Math.min(x - x0, x0 + w - 1 - x);
        const dy = Math.min(y - y0, y0 + h - 1 - y);
        if (dx < r && dy < r && (r - dx) ** 2 + (r - dy) ** 2 > r * r) continue;
      }
      const i = (y * S + x) * 4;
      px[i] = cr; px[i + 1] = cg; px[i + 2] = cb; px[i + 3] = 255;
    }
  }
};

fill(0, 0, S, S, "#0f1216");
const rows = [
  { y: 286, rail: "#3ec9a7", bar: "#36434f", w: 566 },
  { y: 462, rail: "#2f8d78", bar: "#303c47", w: 500 },
  { y: 638, rail: "#e0a34b", bar: "#3a3320", w: 432 },
];
for (const { y, rail, bar, w } of rows) {
  fill(212, y, 44, 120, rail, 14);
  fill(298, y, w, 120, bar, 14);
}

// PNG: one filter byte (0 = none) per scanline, then zlib, then chunks.
const raw = Buffer.alloc(S * (S * 4 + 1));
for (let y = 0; y < S; y++) {
  raw[y * (S * 4 + 1)] = 0;
  Buffer.from(px.buffer, y * S * 4, S * 4).copy(raw, y * (S * 4 + 1) + 1);
}
const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const sum = Buffer.alloc(4);
  sum.writeUInt32BE(crc(body));
  return Buffer.concat([len, body, sum]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;

writeFileSync("src-tauri/icons/source.png", Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]));
console.log("wrote src-tauri/icons/source.png");
