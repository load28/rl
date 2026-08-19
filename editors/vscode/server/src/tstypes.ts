/* --------------------------------------------------------------------------
 * The shapes an answer from TypeScript takes.
 *
 * The server speaks in offsets and spans; who answers — an in-process
 * language service once, the compiler's own language server now — is behind
 * this contract, not in it.
 * ----------------------------------------------------------------------- */

/** A file the editor currently has open, served instead of the disk copy.
 * `version` may carry a suffix distinguishing raw from virtual (emitted)
 * serving of the same buffer version, so the service re-reads on switch. */
export interface OpenDoc {
  text: string;
  version: number | string;
}

/** One definition target, with the target file's text for offset→position
 * conversion (offsets are UTF-16 code units, the LSP default). */
export interface TsDefinition {
  fileName: string;
  start: number;
  length: number;
  fileText: string;
}

/** Quick-info for hover: a code-ish signature plus optional documentation,
 * and the source span it applies to. */
export interface TsQuickInfo {
  signature: string;
  documentation: string;
  start: number;
  length: number;
}

/** One completion entry. `kind` is TypeScript's ScriptElementKind string
 * (e.g. "function", "const", "property") — mapped to an LSP kind by the
 * server so this module stays the only place importing `typescript`.
 * `source`/`data` are TypeScript's own opaque handles for the entry; they
 * are carried back to [`TsProject.completionDetail`] unread. */
export interface TsCompletion {
  name: string;
  kind: string;
  sortText: string;
  source?: string;
  data?: unknown;
}

/** A completion answer: the entries, and whether TypeScript answered as a
 * *member* completion. The flag is what tells an answer at `x |> n.` — where
 * the TS parser lost the dot recovering from `|>` and offered the whole
 * global scope — from a real member list. */
export interface TsCompletionList {
  entries: TsCompletion[];
  member: boolean;
}

/** The type signature and documentation behind one completion entry —
 * what makes `Result.map` in the list show its parameters and return type
 * instead of a bare name. */
export interface TsCompletionDetail {
  /** The declaration as TypeScript displays it, e.g.
   * `(property) map: <T, E, U>(r: Result<T, E>, f: (value: T) => U) => Result<U, E>`. */
  signature: string;
  /** The entry's JSDoc, empty when it has none. */
  documentation: string;
}

/** One overload in a signature help response. Parameter labels are
 * `[start, end)` offsets into `label` (the LSP form that highlights the
 * active parameter inside the rendered signature). */
export interface TsSignature {
  label: string;
  parameters: { label: [number, number]; documentation: string }[];
  documentation: string;
}

/** Signature help at a call site: the overloads plus which of them and
 * which parameter the cursor is in. */
export interface TsSignatureHelp {
  signatures: TsSignature[];
  activeSignature: number;
  activeParameter: number;
}

/** One type error, in the coordinates of the text the service was given
 * (the emitted TypeScript for an `.rl` module — the caller maps it back). */
export interface TsDiagnostic {
  start: number;
  length: number;
  message: string;
  /** TypeScript's error number, e.g. 2345. */
  code: number;
  /** True for `ts.DiagnosticCategory.Warning`; everything reported here is
   * otherwise an error. */
  warning: boolean;
}

/** The name a rename asks TypeScript for, so the answer's `newText` can be
 * read as "the new name, in whatever shape this location needs it". */
export const RENAME_PLACEHOLDER = "rlRenamePlaceholder";

/** One reference or rename location. */
export interface TsReference extends TsDefinition {
  isDefinition: boolean;
  /** Rename only: the text TypeScript wants at this location, with
   * [`RENAME_PLACEHOLDER`] standing in for the new name. Usually the
   * placeholder alone; a destructuring shorthand (`{ value }`, which is
   * what an rl pattern binding compiles to) expands to `value: <new>`
   * instead, and the caller has to keep that expansion to rename the
   * binding without silently rebinding a different field. */
  newText?: string;
}

/** The TypeScript-inferred type at a position (see [`TsProject.typeAt`]):
 * display name without generic arguments, plus the declaring file of its
 * symbol when TypeScript knows one. */
export interface TsTypeInfo {
  name: string;
  declFile: string | null;
}
