import type { Payload } from "./protocol";
/** A disposable browsing context. Only PaneRuntimeManager owns readers. */
export interface ReaderRuntime {
  ready(): Promise<void>;
  command(command: Payload): void;
  dispose(): Promise<void>;
}
