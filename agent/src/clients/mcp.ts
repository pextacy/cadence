import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";

/** A connected MCP session with helpers for tool discovery and typed calls. */
export class McpSession {
  private client: Client;
  private connected = false;
  private toolNames: string[] = [];

  constructor(
    private readonly url: string,
    private readonly auth?: { header: string; value: string },
    name = "cadence-agent",
  ) {
    this.client = new Client({ name, version: "0.1.0" });
  }

  async connect(): Promise<void> {
    if (this.connected) return;
    const requestInit: RequestInit | undefined = this.auth
      ? { headers: { [this.auth.header]: this.auth.value } }
      : undefined;
    const transport = new StreamableHTTPClientTransport(new URL(this.url), { requestInit });
    await this.client.connect(transport);
    const { tools } = await this.client.listTools();
    this.toolNames = tools.map((t) => t.name);
    this.connected = true;
  }

  /** All tool names advertised by the server. */
  tools(): readonly string[] {
    return this.toolNames;
  }

  /**
   * Resolve a tool by trying explicit candidate names first, then a name regex.
   * Throws if none match — the executor treats this as a reason to pause rather
   * than guess.
   */
  resolveTool(candidates: string[], pattern: RegExp): string {
    for (const c of candidates) {
      if (this.toolNames.includes(c)) return c;
    }
    const found = this.toolNames.find((n) => pattern.test(n));
    if (!found) {
      throw new Error(
        `No MCP tool matched ${candidates.join("/")} or ${pattern} on ${this.url}; available: ${this.toolNames.join(", ")}`,
      );
    }
    return found;
  }

  /** Call a tool and return its parsed JSON result (structured content preferred). */
  async call<T>(name: string, args: Record<string, unknown>): Promise<T> {
    const res = await this.client.callTool({ name, arguments: args });
    if (res.isError) {
      throw new Error(`MCP tool ${name} errored: ${JSON.stringify(res.content)}`);
    }
    if (res.structuredContent !== undefined) {
      return res.structuredContent as T;
    }
    const content = res.content as Array<{ type: string; text?: string }> | undefined;
    const text = content?.find((c) => c.type === "text")?.text;
    if (text === undefined) {
      throw new Error(`MCP tool ${name} returned no text/structured content`);
    }
    return JSON.parse(text) as T;
  }

  async close(): Promise<void> {
    if (this.connected) {
      await this.client.close();
      this.connected = false;
    }
  }
}
