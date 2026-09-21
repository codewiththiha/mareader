import type { ReaderRuntime } from "./lifecycle";
import type { Format, Payload, ReaderConfig } from "./protocol";
/** Authoritative ownership map. Disposal acknowledgment precedes deletion. */
export class PaneRuntimeManager {
  readonly panes = new Map<string, ReaderRuntime>();
  private readonly closing = new Map<string, Promise<void>>();
  constructor(private readonly create: (id: string, config: ReaderConfig, event: (event: Payload) => void) => ReaderRuntime,
    private readonly event: (id: string, event: Payload) => void) {}
  async open(id: string, config: ReaderConfig): Promise<void> {
    if (this.panes.has(id) || this.panes.size >= 4) throw new Error("Invalid pane allocation");
    const runtime = this.create(id, config, (event) => {
      if (this.panes.get(id) === runtime) this.event(id, event);
    });
    this.panes.set(id, runtime);
    try {
      await runtime.ready();
      if (this.panes.get(id) === runtime && !this.closing.has(id)) runtime.command({ type: "open", config });
    } catch (error) {
      await this.close(id);
      throw error;
    }
  }
  command(id: string, message: Payload): void { this.panes.get(id)?.command(message); }
  close(id: string): Promise<void> {
    const pending = this.closing.get(id);
    if (pending) return pending;
    const runtime = this.panes.get(id);
    if (!runtime) return Promise.resolve();
    const done = runtime.dispose().finally(() => {
      this.panes.delete(id);
      this.closing.delete(id);
      console.log("[runtime] LIVE PANES", this.panes.size);
    });
    this.closing.set(id, done);
    return done;
  }
  async closeAll(): Promise<void> { await Promise.all([...this.panes.keys()].map((id) => this.close(id))); }
}

/** One pane as the host books it, before the wire shape: the host's own
 *  bookkeeping, nothing the sidebar could not work out itself. */
export interface PaneMeta {
  id: string;
  bookId: string;
  title: string | null;
  format: Format;
  loading: boolean;
  closing: boolean;
}

/** The whole pane set the workspace sidebar derives from: the Active
 *  section shows and hides on this one payload, and the shared Blend paper
 *  rides along so the backdrop never waits for a second channel. */
export function panesPayload(activePaneId: string | null, panes: PaneMeta[], blendPaper: string | null): Payload {
  return {
    type: "panes",
    activePaneId,
    blendPaper,
    panes: panes.map((pane) => ({
      paneId: pane.id,
      bookId: pane.bookId,
      title: pane.title,
      format: pane.format,
      status: pane.closing ? "closing" : pane.loading ? "loading" : "ready",
    })),
  };
}
