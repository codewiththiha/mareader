/** Versioned value-only contract. No DOM nodes, library rows or WASM handles. */
export const VERSION = 1 as const;
export const COMMAND_EVENT = "mareader:runtime-command";
export const OUTPUT_EVENT = "mareader:runtime-event";
export const CONNECTED_EVENT = "mareader:connected";
export type Format = "pdf" | "txt" | "md";
export type RuntimeStatus = "creating" | "loading" | "ready" | "closing" | "disposed" | "failed";
export interface ReaderConfig {
  bookId: string;
  path: string;
  format: Format;
  title: string | null;
  cover: string | null;
  resumePage: number;
  resumeFraction: number | null;
  settings: Record<string, unknown>;
}
export interface Payload { type: string; [key: string]: unknown }
export interface Envelope { version: typeof VERSION; requestId?: number; payload: Payload }
export function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
export function payload(value: unknown): value is Payload {
  return record(value) && typeof value.type === "string";
}
export function envelope(value: unknown): value is Envelope {
  return record(value) && value.version === VERSION && payload(value.payload)
    && (value.requestId === undefined || (Number.isSafeInteger(value.requestId) && Number(value.requestId) > 0));
}
export function readerConfig(value: unknown): value is ReaderConfig {
  return record(value) && typeof value.bookId === "string" && value.bookId.length > 0
    && typeof value.path === "string" && value.path.length > 0
    && ["pdf", "txt", "md"].includes(String(value.format))
    && (value.title === null || typeof value.title === "string")
    && (value.cover === null || typeof value.cover === "string")
    && Number.isInteger(value.resumePage) && Number(value.resumePage) >= 1
    && (value.resumeFraction === null || (typeof value.resumeFraction === "number"
      && Number.isFinite(value.resumeFraction) && value.resumeFraction >= 0 && value.resumeFraction <= 1))
    && record(value.settings);
}
export function packet(value: Payload, requestId?: number): Envelope {
  return { version: VERSION, requestId, payload: value };
}
export function localCommand(value: Payload): void {
  window.dispatchEvent(new CustomEvent(COMMAND_EVENT, { detail: value }));
}
