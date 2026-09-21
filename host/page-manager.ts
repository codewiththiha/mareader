// Host-owned route state machine — the only place allowed to switch top-level WASM.
// Target graph per plan §3/§4:
//   Library (library.wasm iframe)
//     │ open book
//     ▼
//   Reader  (reader-pdf/txt/md.wasm iframe, no workspace)
//     │ enter split
//     ▼
//   Workspace (workspace.wasm + one reader iframe per pane, max four)
//     │ close all / return
//     ▼
//   Library
//
// The current product boots Workspace directly (index.html embeds workspace-wasm
// in the host document). This manager preserves that boot path while providing
// the lifecycle hooks to destroy/restore each realm correctly when the product
// enables the full isolation model. The hard boundary is always:
//   Rust cleanup + MessagePort close + listeners/timers/observers + iframe.src=about:blank + iframe.remove + JS null.

import { FrameRuntime } from "./frame";
import { PaneRuntimeManager } from "./panes";
import type { ReaderConfig, Payload, Format } from "./protocol";

export type Route =
  | { kind: "library" }
  | { kind: "reader"; format: Format; config: ReaderConfig }
  | { kind: "workspace" };

export interface PageState {
  kind: Route["kind"];
  generation: string;
}

export class PageManager {
  private generation = this.newGeneration();
  // Kept for route assertions and future Library → Reader → Workspace state machine.
  // Referenced in navigation methods; the explicit field satisfies exhaustive route checks.
  private current: Route = { kind: "workspace" };
  private libraryRuntime: FrameRuntime | null = null;
  private readerRuntime: FrameRuntime | null = null;
  private workspacePanes: PaneRuntimeManager | null = null;
  private readonly appRoot: HTMLElement;

  constructor(appRoot: HTMLElement) {
    this.appRoot = appRoot;
  }

  private newGeneration(): string {
    return (globalThis.crypto as unknown as { randomUUID?: () => string })?.randomUUID?.() ??
      [...crypto.getRandomValues(new Uint32Array(4))].map(v => v.toString(16).padStart(8, "0")).join("");
  }

  get route(): Route {
    return this.current;
  }

  /** Active generation — each route/pane message carries this; host ignores stale ones. */
  get activeGeneration(): string {
    return this.generation;
  }

  /** True if payload generation matches current; otherwise stale async render. */
  isCurrent(payload: Payload & { generation?: string }): boolean {
    return !payload.generation || payload.generation === this.generation;
  }

  /** Library → destroy library realm, clear references, GC can reclaim linear memory. */
  async showLibrary(): Promise<void> {
    await this.disposeReader();
    await this.disposeWorkspace();
    await this.disposeLibrary();
    this.generation = this.newGeneration();
    this.current = { kind: "library" };
    // Library is an iframe realm — its linear memory starts fresh.
    const container = this.ensureContainer("library-root");
    const config: ReaderConfig = {
      bookId: "library",
      path: "__library__",
      format: "pdf",
      title: "Library",
      cover: null,
      resumePage: 1,
      resumeFraction: null,
      settings: {} as unknown as Record<string, unknown>,
    };
    this.libraryRuntime = new FrameRuntime("library", container, config, () => {}, async () => {}, "library");
    await this.libraryRuntime.ready();
  }

  /** Single reader — only the selected format's WASM is instantiated, no workspace. */
  async showReader(config: ReaderConfig): Promise<void> {
    await this.disposeLibrary();
    await this.disposeWorkspace();
    await this.disposeReader();
    this.generation = this.newGeneration();
    this.current = { kind: "reader", format: config.format as Format, config };
    const container = this.ensureContainer("reader-root");
    this.readerRuntime = new FrameRuntime(`reader-${config.format}`, container, config, () => {}, async () => {});
    await this.readerRuntime.ready();
    this.readerRuntime.command({ type: "open", config } as unknown as Payload);
  }

  /** Enter split — snapshot current reader, dispose it, create workspace + first pane. */
  async enterWorkspace(fromReader?: ReaderConfig): Promise<{ manager: PaneRuntimeManager }> {
    await this.disposeLibrary();
    await this.disposeReader();
    if (!this.workspacePanes) {
      this.generation = this.newGeneration();
      this.current = { kind: "workspace" };
      // Workspace WASM itself is the host document's mount (Trunk's rust link).
      // Its realm is the host realm; panes are the isolated child realms.
      this.workspacePanes = new PaneRuntimeManager(
        (id, cfg, ev) => {
          const slot = document.querySelector(`[data-pane="${id}"]`) as HTMLElement ?? this.ensurePaneSlot(id);
          return new FrameRuntime(id, slot, cfg, ev, async () => {});
        },
        () => {},
      );
      if (fromReader) {
        const id = `pane-${Date.now()}`;
        await this.workspacePanes.open(id, fromReader);
      }
    }
    return { manager: this.workspacePanes };
  }

  /** Workspace → Library — destroy every pane first, then workspace, then mount library. */
  async workspaceToLibrary(): Promise<void> {
    await this.disposeReader();
    if (this.workspacePanes) {
      await this.workspacePanes.closeAll();
      this.workspacePanes = null;
      // Workspace's own Rust unmount: clear host container and signal GC.
      const root = document.getElementById("workspace-root");
      if (root) root.replaceChildren();
    }
    await this.showLibrary();
  }

  private ensureContainer(id: string): HTMLElement {
    let el = document.getElementById(id);
    if (!el) {
      el = document.createElement("div");
      el.id = id;
      el.style.cssText = "position:absolute;inset:0;";
      this.appRoot.append(el);
    }
    return el as HTMLElement;
  }

  private ensurePaneSlot(id: string): HTMLElement {
    let layout = document.getElementById("workspace-layout");
    if (!layout) {
      layout = document.createElement("div");
      layout.id = "workspace-layout";
      layout.className = "relative min-w-0 flex-1 overflow-hidden";
      this.appRoot.append(layout);
    }
    let slot = layout.querySelector(`[data-pane="${id}"]`) as HTMLElement | null;
    if (!slot) {
      slot = document.createElement("div");
      slot.className = "workspace-pane";
      slot.dataset.pane = id;
      layout.append(slot);
    }
    return slot;
  }

  private async disposeLibrary(): Promise<void> {
    if (this.libraryRuntime) {
      await this.libraryRuntime.dispose().catch(() => undefined);
      this.libraryRuntime = null;
      const root = document.getElementById("library-root");
      if (root) { root.replaceChildren(); root.remove(); }
    }
  }

  private async disposeReader(): Promise<void> {
    if (this.readerRuntime) {
      await this.readerRuntime.dispose().catch(() => undefined);
      this.readerRuntime = null;
      const root = document.getElementById("reader-root");
      if (root) { root.replaceChildren(); root.remove(); }
    }
  }

  private async disposeWorkspace(): Promise<void> {
    if (this.workspacePanes) {
      await this.workspacePanes.closeAll();
      this.workspacePanes = null;
    }
  }

  /** Debug helper: log host memory including iframes/workers where supported. */
  async logMemory(label: string): Promise<void> {
    const fn = (performance as Performance & { measureUserAgentSpecificMemory?: () => Promise<unknown> }).measureUserAgentSpecificMemory;
    if (!fn) return;
    try {
      const sample = await fn.call(performance);
      console.log(`[memory:${label}]`, sample);
    } catch {}
  }
}
