// The error envelope every engine entry point returns on failure, and the one
// place a thrown value is turned into one. Kept out of `canvas.ts`, which is
// about backing stores: four modules import these and none of them for a
// canvas.

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

/** The catch half of an engine entry point: whatever threw becomes an error
 * envelope, keeping pdf.js's own `name` so a caller can still branch on
 * "PasswordException" and friends. Ten entry points spelled this out as
 * `errorInfo` followed by `fail`, which was ten chances to drop the name. */
export function failFrom(e: unknown): { ok: false; error: { name: string; message: string } } {
  const info = errorInfo(e);
  return fail(info.name, info.message);
}
