/* --------------------------------------------------------------------------
 * Completion probes — types for a construct the user has not finished.
 *
 * The TypeScript language service answers over the *emitted* TypeScript of
 * an .rl buffer (tsproject.ts, virtual.ts): rl syntax is nothing its parser
 * can resolve through — a `|>` pipeline collapses the statement it is in,
 * and a `match` arm's pattern bindings do not exist as declarations at all.
 *
 * That works for a buffer that compiles. It does not work at the instant
 * completion is asked for: `x |> .` has no member yet, so there is nothing
 * to compile into a member access — the compiler passes the file through
 * unchanged and the service is handed raw rl text, where it has nothing to
 * say. The editor then caches that empty list and the members never appear.
 *
 * A probe is the buffer with a placeholder identifier inserted at the
 * cursor: `x |> .$rl_probe` is a whole method step, compiles like any
 * pipeline, and the placeholder's mapped position in the emitted output is
 * exactly where the piped value's members are asked for.
 *
 * This module owns building one (a pure text edit plus the offset mapping);
 * running the compiler and serving the result to the language service is
 * the server's. A probe is never anything but a completion device — no
 * diagnostic may be computed from text the user did not write.
 * ----------------------------------------------------------------------- */
import { MappedDoc } from "./virtual";

/** Inserted at the cursor to complete the construct being typed. `$`-led so
 * it cannot collide with the name the user is in the middle of typing. */
export const PROBE_NAME = "$rl_probe";

/** An emit map for probe source, as `rlc --emit-map` produces it. */
export interface EmitMap {
  code: string;
  mappings: { src: number; out: number; len: number }[];
}

/** The compiled probe: emitted TypeScript, and where in it to ask. */
export interface Probe {
  /** Emitted TypeScript of the probe source. */
  code: string;
  /** The placeholder's position in `code` — where the service is asked. */
  offset: number;
}

/** The buffer with the placeholder inserted at `offset`. */
export function probeSource(text: string, offset: number): string {
  return text.slice(0, offset) + PROBE_NAME + text.slice(offset);
}

/**
 * True when `offset` sits right after a member-access dot — the one
 * position a placeholder can complete. `masked` must be the masked source
 * (analysis.ts), so a `.` inside a string, comment or regex is not one.
 */
export function atMemberAccess(masked: string, offset: number): boolean {
  return offset > 0 && masked[offset - 1] === ".";
}

/**
 * Compile a probe for the cursor with `emit`, and locate the placeholder in
 * the output. Null when the compiler cannot emit for the probe source
 * either — the buffer is broken somewhere a placeholder does not reach —
 * or when the placeholder's position has no counterpart in the output.
 */
export async function buildProbe(
  text: string,
  offset: number,
  emit: (source: string) => Promise<EmitMap | null>,
): Promise<Probe | null> {
  const source = probeSource(text, offset);
  const result = await emit(source);
  if (!result) return null;
  let at: number | null;
  try {
    at = new MappedDoc(source, result.code, result.mappings).srcToOut(offset);
  } catch {
    return null;
  }
  return at === null ? null : { code: result.code, offset: at };
}
