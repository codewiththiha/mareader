// The desktop boot path has one persistent chrome root and one lifecycle owner.
// PageManager is the single authority for top-level WASM lifetimes (plan §3).
import { bootWorkspace } from "./workspace-host";
import { PageManager } from "./page-manager";
import { logMemory } from "./memory";

const appRoot = document.getElementById("workspace-root");
if (appRoot) {
  const manager = new PageManager(appRoot);
  (window as unknown as Record<string, unknown>).__MAREADER_PAGE__ = manager;
  void logMemory("host-boot");
  void manager.logMemory("workspace-boot");
}

bootWorkspace();
