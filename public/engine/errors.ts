// The error envelope every engine entry point returns.

/** An error envelope: `{ ok: false, error: { name, message } }`. */
export function fail(name: string, message: string): { ok: false; error: { name: string; message: string } } {
  return { ok: false, error: { name, message } };
}

export function errorInfo(e: unknown): { name: string; message: string } {
  const er = e as { name?: string; message?: string } | null;
  const name = (er && er.name) || "Error";
  const message = (er && er.message) || String(e);
  return { name, message };
}

// The catch half: whatever threw becomes an envelope, keeping `name`.
export function failFrom(e: unknown): { ok: false; error: { name: string; message: string } } {
  const info = errorInfo(e);
  return fail(info.name, info.message);
}
