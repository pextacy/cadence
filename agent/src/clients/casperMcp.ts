import { McpSession } from "./mcp.js";

/** Status of a submitted deploy/transaction as reported by the Casper MCP. */
export interface DeployStatus {
  hash: string;
  executed: boolean;
  success: boolean;
}

/**
 * Reads over the Casper MCP Server: balances and deploy confirmations. Optional —
 * only constructed when `CASPER_MCP_URL` is configured. Tool names are discovered
 * at connect time.
 */
export class CasperMcpClient {
  private session: McpSession;

  constructor(url: string) {
    this.session = new McpSession(url, undefined, "cadence-casper-mcp");
  }

  async connect(): Promise<void> {
    await this.session.connect();
  }

  /** Confirm a deploy/transaction by hash. */
  async getDeployStatus(hash: string): Promise<DeployStatus> {
    const tool = this.session.resolveTool(
      ["get_deploy", "get_transaction", "deploy_status"],
      /deploy|transaction/i,
    );
    const raw = await this.session.call<{
      executed?: boolean;
      success?: boolean;
      execution_result?: { success?: boolean };
    }>(tool, { hash });
    const success = raw.success ?? raw.execution_result?.success ?? false;
    return { hash, executed: raw.executed ?? success, success };
  }

  async close(): Promise<void> {
    await this.session.close();
  }
}
