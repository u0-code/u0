// One connection to a daemon: JSON-RPC 2.0 over a WebSocket, one object per
// text frame. Reconnects with backoff until told to stop or refused for a
// reason that retrying cannot fix.

import { type Frame, type Hello, type Welcome } from "./types";

export type LinkState =
  | { is: "connecting" }
  | { is: "open"; welcome: Welcome }
  | { is: "closed"; why: string }
  /** The daemon said no: a wrong token, a taken channel. */
  | { is: "refused"; why: string; code: number };

const UNAUTHORIZED = -32001;
const VERSION = -32002;

type Waiter = { resolve: (value: unknown) => void; reject: (error: Error) => void };

export class Link {
  /** A notification from the daemon. */
  onNotify: (method: string, params: unknown) => void = () => {};
  /** A request from the daemon. Whatever this returns is the result. */
  onRequest: (method: string, params: unknown) => Promise<unknown> = async () => ({});
  onState: (state: LinkState) => void = () => {};

  private ws: WebSocket | null = null;
  private nextId = 1;
  private waiting = new Map<number, Waiter>();
  private retry = 1000;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;
  private open = false;
  private refused = false;

  constructor(
    private readonly url: string,
    private readonly hello: Hello,
  ) {}

  start() {
    this.stopped = false;
    this.connect();
  }

  stop() {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.ws?.close();
    this.ws = null;
  }

  get isOpen() {
    return this.open;
  }

  call<T>(method: string, params?: unknown): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error("not connected"));
        return;
      }
      const id = this.nextId++;
      this.waiting.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.write({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method: string, params?: unknown) {
    this.write({ jsonrpc: "2.0", method, params });
  }

  private write(frame: Frame) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(frame));
  }

  private connect() {
    if (this.stopped) return;
    this.onState({ is: "connecting" });
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch (error) {
      this.onState({ is: "closed", why: String(error) });
      this.again();
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      this.call<Welcome>("hello", this.hello).then(
        (welcome) => {
          this.open = true;
          this.retry = 1000;
          this.onState({ is: "open", welcome });
        },
        (error: Error & { code?: number }) => {
          const code = error.code ?? 0;
          this.refused = true;
          this.onState({ is: "refused", why: error.message, code });
          // A wrong token or protocol will be just as wrong next time.
          if (code === UNAUTHORIZED || code === VERSION) this.stopped = true;
          ws.close();
        },
      );
    };
    ws.onmessage = (message) => {
      let frame: Frame;
      try {
        frame = JSON.parse(String(message.data)) as Frame;
      } catch {
        return;
      }
      this.receive(frame);
    };
    ws.onclose = () => {
      const wasOpen = this.open;
      const refused = this.refused;
      this.open = false;
      this.refused = false;
      for (const waiter of this.waiting.values()) waiter.reject(new Error("connection closed"));
      this.waiting.clear();
      if (this.ws === ws) this.ws = null;
      if (this.stopped) return;
      // A refusal has already been reported; do not paper over it.
      if (wasOpen) this.onState({ is: "closed", why: "the daemon went away" });
      else if (!refused) this.onState({ is: "closed", why: "cannot reach the daemon" });
      this.again();
    };
  }

  private again() {
    if (this.stopped) return;
    this.timer = setTimeout(() => this.connect(), this.retry);
    this.retry = Math.min(this.retry * 2, 15000);
  }

  private receive(frame: Frame) {
    const isResponse = frame.method === undefined && frame.id !== undefined;
    if (isResponse) {
      const waiter = this.waiting.get(frame.id!);
      if (!waiter) return;
      this.waiting.delete(frame.id!);
      if (frame.error) {
        const error = new Error(frame.error.message) as Error & { code?: number };
        error.code = frame.error.code;
        waiter.reject(error);
      } else {
        waiter.resolve(frame.result);
      }
      return;
    }
    if (frame.method === undefined) return;
    if (frame.id === undefined) {
      this.onNotify(frame.method, frame.params);
      return;
    }
    const id = frame.id;
    this.onRequest(frame.method, frame.params).then(
      (result) => this.write({ jsonrpc: "2.0", id, result: result ?? {} }),
      (error: unknown) =>
        this.write({ jsonrpc: "2.0", id, error: { code: -32010, message: String(error) } }),
    );
  }
}
