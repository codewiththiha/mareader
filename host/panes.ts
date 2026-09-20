import type { ReaderRuntime } from "./lifecycle";
import type { Payload, ReaderConfig } from "./protocol";
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
    const done = runtime.dispose().finally(() => { this.panes.delete(id); this.closing.delete(id); });
    this.closing.set(id, done);
    return done;
  }
  async closeAll(): Promise<void> { await Promise.all([...this.panes.keys()].map((id) => this.close(id))); }
}
