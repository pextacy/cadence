/** A streaming message envelope from CSPR.cloud. */
export interface StreamMessage<T = unknown> {
  action: string;
  data: T;
  timestamp?: string;
  extra?: unknown;
}

/**
 * Minimal CSPR.cloud REST client. All requests carry the `Authorization` header
 * with the access token. Responses wrap their payload in a `data` field.
 */
export class CsprCloudRest {
  constructor(
    private readonly baseUrl: string,
    private readonly apiKey: string,
  ) {}

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      headers: { Authorization: this.apiKey },
    });
    if (!res.ok) {
      throw new Error(`CSPR.cloud ${path} failed: ${res.status}`);
    }
    const body = (await res.json()) as { data: T };
    return body.data;
  }

  /** Read an account's main-purse CSPR balance in motes. */
  async getAccountBalance(accountHashOrKey: string): Promise<bigint> {
    const data = await this.get<{ balance: string }>(
      `/accounts/${encodeURIComponent(accountHashOrKey)}`,
    );
    return BigInt(data.balance);
  }

  /** Read indexed contract state (returns the raw `data` payload). */
  async getContract<T>(contractHash: string): Promise<T> {
    return this.get<T>(`/contracts/${encodeURIComponent(contractHash)}`);
  }
}

/**
 * Subscribe to a CSPR.cloud streaming channel over WebSocket. Returns a function
 * that closes the connection. Node 22+ and browsers provide a global `WebSocket`.
 */
export function subscribeStream<T = unknown>(opts: {
  streamingUrl: string;
  apiKey: string;
  channel: string;
  onMessage: (msg: StreamMessage<T>) => void;
  onError?: (err: unknown) => void;
}): () => void {
  const url = `${opts.streamingUrl}${opts.channel}`;
  // The WHATWG WebSocket has no header support, so the access token is sent as
  // the first subscription message after the connection opens.
  const ws = new WebSocket(url);

  ws.addEventListener("open", () => {
    ws.send(JSON.stringify({ action: "subscribe", token: opts.apiKey }));
  });
  ws.addEventListener("message", (ev: MessageEvent) => {
    try {
      const text = typeof ev.data === "string" ? ev.data : String(ev.data);
      opts.onMessage(JSON.parse(text) as StreamMessage<T>);
    } catch (err) {
      opts.onError?.(err);
    }
  });
  ws.addEventListener("error", (ev) => opts.onError?.(ev));

  return () => ws.close();
}
