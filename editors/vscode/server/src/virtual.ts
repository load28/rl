/* --------------------------------------------------------------------------
 * Virtual-document offset mapping (TASK-050).
 *
 * `rlc --emit-map` gives us the emitted TypeScript of an .rl buffer plus
 * byte mappings for every chunk copied verbatim from the source. Serving
 * that emitted code to the TypeScript language service (instead of the raw
 * .rl text) gives TS a file with no rl syntax in it — full inference inside
 * match arms, scrutinees, try/let-else/if-let expressions and pipelines.
 *
 * The compiler speaks bytes; the language service speaks UTF-16 code units.
 * MappedDoc owns both conversions: source-UTF-16 → output-UTF-16 for
 * queries, and back for result spans. Offsets that fall in compiler glue
 * (IIFE scaffolding, destructurings) have no mapping and return null —
 * callers degrade to no answer rather than a wrong one.
 * ----------------------------------------------------------------------- */

/** One verbatim-copied chunk: `len` *bytes* from source offset `src`
 * appear at output offset `out` (the JSON `rlc --emit-map` prints). */
export interface EmitMapping {
  src: number;
  out: number;
  len: number;
}

/** Byte ↔ UTF-16 offset conversion for one text. ASCII texts (the common
 * case) convert by identity without allocating tables. */
class OffsetConverter {
  private readonly ascii: boolean;
  /** byte offset for each UTF-16 index (length + 1 entries). */
  private byteAt: Uint32Array | null = null;
  /** UTF-16 offset for each byte index (byteLength + 1 entries). */
  private u16At: Uint32Array | null = null;

  constructor(text: string) {
    // eslint-disable-next-line no-control-regex
    this.ascii = !/[^\x00-\x7f]/.test(text);
    if (this.ascii) return;

    const byteAt = new Uint32Array(text.length + 1);
    let byte = 0;
    let i = 0;
    const u16Marks: { byte: number; u16: number }[] = [];
    while (i < text.length) {
      const cp = text.codePointAt(i)!;
      const u16len = cp > 0xffff ? 2 : 1;
      const byteLen = cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4;
      for (let k = 0; k < u16len; k++) byteAt[i + k] = byte;
      u16Marks.push({ byte, u16: i });
      byte += byteLen;
      i += u16len;
    }
    byteAt[text.length] = byte;

    const u16At = new Uint32Array(byte + 1);
    for (let k = 0; k < u16Marks.length; k++) {
      // bytes inside a character map to the character's start
      const end = k + 1 < u16Marks.length ? u16Marks[k + 1].byte : byte;
      u16At.fill(u16Marks[k].u16, u16Marks[k].byte, end);
    }
    u16At[byte] = text.length;
    this.byteAt = byteAt;
    this.u16At = u16At;
  }

  toByte(u16: number): number {
    if (this.ascii) return u16;
    const table = this.byteAt!;
    const clamped = Math.max(0, Math.min(u16, table.length - 1));
    return table[clamped];
  }

  toU16(byte: number): number {
    if (this.ascii) return byte;
    const table = this.u16At!;
    const clamped = Math.max(0, Math.min(byte, table.length - 1));
    return table[clamped];
  }
}

/** An .rl buffer paired with its emitted TypeScript and offset mappings. */
export class MappedDoc {
  private readonly srcConv: OffsetConverter;
  private readonly outConv: OffsetConverter;
  private readonly bySrc: EmitMapping[];
  private readonly byOut: EmitMapping[];

  constructor(
    /** The .rl source text this mapping was computed from. */
    public readonly srcText: string,
    /** The emitted TypeScript served to the language service. */
    public readonly code: string,
    mappings: EmitMapping[],
  ) {
    this.srcConv = new OffsetConverter(srcText);
    this.outConv = new OffsetConverter(code);
    this.bySrc = [...mappings].sort((a, b) => a.src - b.src);
    this.byOut = [...mappings].sort((a, b) => a.out - b.out);
  }

  /** The chunk containing `offset` (in the `key` coordinate). A chunk's
   * end position belongs to it too unless the next chunk starts there —
   * completion offsets sit at the end of what was just typed. */
  private chunkAt(
    sorted: EmitMapping[],
    key: "src" | "out",
    offset: number,
  ): EmitMapping | null {
    let lo = 0;
    let hi = sorted.length - 1;
    let candidate: EmitMapping | null = null;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const m = sorted[mid];
      if (m[key] > offset) {
        hi = mid - 1;
      } else {
        candidate = m;
        lo = mid + 1;
      }
    }
    if (candidate && offset <= candidate[key] + candidate.len) return candidate;
    return null;
  }

  /** Source UTF-16 offset → emitted-output UTF-16 offset, or null when the
   * position was not copied into the output. */
  srcToOut(offset: number): number | null {
    const byte = this.srcConv.toByte(offset);
    const m = this.chunkAt(this.bySrc, "src", byte);
    if (!m) return null;
    return this.outConv.toU16(m.out + (byte - m.src));
  }

  /** Emitted-output UTF-16 offset → source UTF-16 offset, or null when the
   * position is compiler glue with no source counterpart. */
  outToSrc(offset: number): number | null {
    const byte = this.outConv.toByte(offset);
    const m = this.chunkAt(this.byOut, "out", byte);
    if (!m) return null;
    return this.srcConv.toU16(m.src + (byte - m.out));
  }
}
