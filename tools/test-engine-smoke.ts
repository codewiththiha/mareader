// Entry point: run every engine-smoke scenario in document order.

(async () => {
  await (await import("./engine-smoke/raster-lane.js")).run();
  await (await import("./engine-smoke/open.js")).run();
  await (await import("./engine-smoke/render.js")).run();
  await (await import("./engine-smoke/theme.js")).run();
  await (await import("./engine-smoke/thumbnail.js")).run();
  await (await import("./engine-smoke/blend.js")).run();
  await (await import("./engine-smoke/search.js")).run();
  await (await import("./engine-smoke/selection.js")).run();
  await (await import("./engine-smoke/sessions.js")).run();
  await (await import("./engine-smoke/teardown.js")).run();
  console.log("ALL ENGINE TESTS PASSED");
})().catch((e: unknown) => {
  console.error("TEST FAILURE:", e);
  process.exit(1);
});
