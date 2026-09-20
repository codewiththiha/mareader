import type { ReaderConfig, Payload, RuntimeStatus } from "./protocol";

export interface ReaderRuntime {
  ready(): Promise<void>;
  command(command: Payload): void;
  dispose(): Promise<void>;
}
export type RuntimeFactory = (config: ReaderConfig, event: (event: Payload) => void) => ReaderRuntime;

/** The only strong reference to a reader. Generations protect both sides of
 * every await; a new frame cannot be created until the old one is destroyed. */
export class HostRuntimeManager {
  private current: ReaderRuntime | null = null;
  private generation = 0;
  private closing: Promise<void> = Promise.resolve();
  private status: RuntimeStatus = "disposed";
  constructor(
    private readonly create: RuntimeFactory,
    private readonly event: (event: Payload) => void,
    private readonly changed: (status: RuntimeStatus) => void,
  ) {}
  get state(): RuntimeStatus { return this.status; }
  private publish(status: RuntimeStatus): void {
    this.status = status;
    this.changed(status);
  }
  private detach(): Promise<void> {
    const previous = this.current;
    this.current = null;
    this.closing = Promise.all([this.closing, previous?.dispose()]).then(() => undefined);
    return this.closing;
  }
  async open(config: ReaderConfig): Promise<void> {
    const generation = ++this.generation;
    this.publish("closing");
    await this.detach();
    if (generation !== this.generation) return;
    this.publish("creating");
    let runtime: ReaderRuntime | null = null;
    try {
      runtime = this.create(config, (event) => {
        // Deliver final progress while disposing too. A runtime removed by a
        // newer generation can still flush its own book, but cannot navigate.
        if (["progress", "metadata", "cover", "settings"].includes(event.type)
          || generation === this.generation) this.event(event);
      });
      this.current = runtime;
      this.publish("loading");
      await runtime.ready();
      if (generation !== this.generation) return;
      runtime.command({ type: "open", config });
      this.publish("ready");
    } catch (error) {
      if (runtime) await runtime.dispose();
      if (generation !== this.generation) return;
      this.current = null;
      this.publish("failed");
      this.event({ type: "error", message: String(error) });
    }
  }
  async close(): Promise<void> {
    const generation = ++this.generation;
    this.publish("closing");
    await this.detach();
    if (generation === this.generation) this.publish("disposed");
  }
  command(command: Payload): void { this.current?.command(command); }
}
