// The handoff decisions, against the module bundle-engine emits. Run after
// `npm run build:ts`. Node, not a browser: the module has no DOM.

import assert from "node:assert/strict";
import { decideSession, HANDOFF_KEY, navigation, parseHandoff, withSession } from "../scripts/session-handoff.js";

const origin = "http://127.0.0.1:1420/";

assert.equal(HANDOFF_KEY, "mareader.handoff");
assert.equal(HANDOFF_KEY.includes(":"), false);

assert.equal(parseHandoff(null), null);
assert.equal(parseHandoff("not json"), null);
assert.equal(parseHandoff("{}"), null);
assert.equal(parseHandoff(JSON.stringify({ path: "" })), null);
assert.deepEqual(parseHandoff(JSON.stringify({ path: "/tmp/a.pdf" })), { path: "/tmp/a.pdf" });
assert.deepEqual(parseHandoff(JSON.stringify({ path: "/tmp/a.pdf", bookId: "b1" })), {
  path: "/tmp/a.pdf",
  bookId: "b1",
});
assert.deepEqual(parseHandoff(JSON.stringify({ path: "/tmp/a.pdf", bookId: "" })), {
  path: "/tmp/a.pdf",
});

assert.equal(decideSession("", null), "library");
assert.equal(decideSession("?session=library", null), "library");
assert.equal(decideSession("?session=reader", null), "library");
assert.equal(decideSession("?session=reader", "nope"), "library");
assert.equal(
  decideSession("?session=reader", JSON.stringify({ path: "/tmp/a.pdf" })),
  "reader",
);
assert.equal(decideSession("?other=1", JSON.stringify({ path: "/tmp/a.pdf" })), "library");

const shelf = withSession(origin, "library");
const reader = withSession(origin, "reader");
assert.equal(new URL(reader).searchParams.get("session"), "reader");
assert.equal(new URL(reader).pathname, "/");
assert.equal(new URL(shelf).searchParams.get("session"), null);
assert.equal(withSession(reader, "library"), shelf);

assert.equal(navigation(origin, reader, "push"), "push");
assert.equal(navigation(reader, shelf, "replace"), "replace");
assert.equal(navigation(reader, withSession(reader, "reader"), "push"), "reload");

console.log("session handoff ok");
