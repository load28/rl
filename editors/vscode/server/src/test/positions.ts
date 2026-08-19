/* Offset ↔ position conversions for tests: the engine speaks LSP
 * line/character positions over the `.rl` source, while fixtures address
 * text by `indexOf` offset. UTF-16 semantics, like the protocol's. */
import type { EnginePosition, EngineRange } from "../engine";

/** The zero-based line/character an offset names in `text`. */
export function positionAt(text: string, offset: number): EnginePosition {
  const clamped = Math.max(0, Math.min(offset, text.length));
  const before = text.slice(0, clamped);
  const line = before.split("\n").length - 1;
  return { line, character: clamped - (before.lastIndexOf("\n") + 1) };
}

/** The offset a zero-based line/character names in `text`. */
export function offsetAt(text: string, position: EnginePosition): number {
  let offset = 0;
  for (let line = 0; line < position.line; line++) {
    const next = text.indexOf("\n", offset);
    if (next < 0) return text.length;
    offset = next + 1;
  }
  return Math.min(offset + position.character, text.length);
}

/** A range as the `[start, end)` offsets it covers in `text`. */
export function spanOf(
  text: string,
  range: EngineRange,
): { start: number; end: number } {
  return { start: offsetAt(text, range.start), end: offsetAt(text, range.end) };
}

/** The text a range covers. */
export function sliceOf(text: string, range: EngineRange): string {
  const { start, end } = spanOf(text, range);
  return text.slice(start, end);
}
