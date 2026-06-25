import casper from "casper-js-sdk";
import type * as Casper from "casper-js-sdk";
import { McpSession } from "./mcp.js";
import type { Quote } from "../types.js";

// casper-js-sdk is CommonJS; destructure the runtime values from the default export.
const { Deploy, KeyAlgorithm, PrivateKey, Transaction } = casper;

/** Raw quote fields the CSPR.trade MCP may return; mapped into {@link Quote}. */
interface RawQuote {
  amount_out?: string | number;
  amountOut?: string | number;
  out_amount?: string | number;
  venue_address?: string;
  venueAddress?: string;
  pool?: string;
  route_id?: string;
  routeId?: string;
}

/**
 * The quote's *expected* output for the requested input. Only the true expected-out
 * fields are accepted — never a `min_received`/slippage floor, which would understate
 * the quote and trigger false slippage rejections (the floor is derived separately
 * from the mandate's slippage cap, not taken from the venue).
 */
function pickAmount(q: RawQuote): bigint {
  const v = q.amount_out ?? q.amountOut ?? q.out_amount;
  if (v === undefined) {
    throw new Error("CSPR.trade quote did not include an expected output amount (amount_out)");
  }
  return BigInt(typeof v === "number" ? Math.trunc(v).toString() : v);
}

/**
 * Thin adapter over the CSPR.trade MCP for quotes, routes and swap submission.
 * Tool names are discovered at connect time, so the client adapts to the server's
 * actual schema rather than hardcoding names.
 */
export class CsprTradeClient {
  private session: McpSession;
  /** Agent key — signs the unsigned deploy `build_swap` returns (non-custodial:
   * the MCP builds remotely, we sign locally, it submits). */
  private readonly key: Casper.PrivateKey;
  private readonly senderPublicKeyHex: string;

  constructor(
    private readonly url: string,
    private readonly venue: string,
    agentPrivateKeyHex: string,
  ) {
    this.session = new McpSession(url, undefined, "cadence-cspr-trade");
    this.key = PrivateKey.fromHex(agentPrivateKeyHex.replace(/^0x/, ""), KeyAlgorithm.SECP256K1);
    this.senderPublicKeyHex = this.key.publicKey.toHex();
  }

  async connect(): Promise<void> {
    await this.session.connect();
  }

  /**
   * Fetch a fresh quote for selling `amount` of `tokenIn` into `tokenOut`. An
   * optional `venue` hint requests a specific venue/pool and labels the returned
   * quote with it; without a hint the server's default route is used.
   */
  async getQuote(params: {
    tokenIn: string;
    tokenOut: string;
    amount: bigint;
    venue?: string;
  }): Promise<Quote> {
    // Confirmed CSPR.trade MCP schema: get_quote(token_in, token_out, amount, type).
    // It auto-routes (no venue arg); the venue label is carried through for the
    // mandate allowlist / best-execution bookkeeping only.
    const tool = this.session.resolveTool(["get_quote"], /^get_quote$/);
    const raw = await this.session.call<RawQuote>(tool, {
      token_in: params.tokenIn,
      token_out: params.tokenOut,
      amount: params.amount.toString(),
      type: "exact_in",
    });
    const venueAddress = raw.venue_address ?? raw.venueAddress ?? raw.pool;
    if (!venueAddress) {
      throw new Error("CSPR.trade quote did not include a venue/pool address");
    }
    return {
      venue: params.venue ?? this.venue,
      venueAddress,
      sellAmount: params.amount,
      quotedOut: pickAmount(raw),
      // Stamp at receipt so the executor's freshness guard can reject a quote that
      // has gone stale before it is committed on-chain.
      quotedAtMs: Date.now(),
      ...(raw.route_id ?? raw.routeId ? { routeId: raw.route_id ?? raw.routeId } : {}),
    };
  }

  /**
   * Fetch quotes across several candidate venues for the same swap, for
   * best-execution selection. Venues that fail to quote are skipped; the result
   * preserves request order. Throws only if no venue produced a quote.
   */
  async getQuotes(
    params: { tokenIn: string; tokenOut: string; amount: bigint },
    venues: readonly string[],
  ): Promise<Quote[]> {
    const targets = venues.length > 0 ? venues : [this.venue];
    const settled = await Promise.allSettled(
      targets.map((venue) => this.getQuote({ ...params, venue })),
    );
    const quotes = settled
      .filter((r): r is PromiseFulfilledResult<Quote> => r.status === "fulfilled")
      .map((r) => r.value);
    if (quotes.length === 0) {
      throw new Error("CSPR.trade returned no quotes for any allowlisted venue");
    }
    return quotes;
  }

  /**
   * Execute a swap through the CSPR.trade MCP's non-custodial flow: `build_swap`
   * returns an UNSIGNED deploy, we sign it locally with the agent key, and
   * `submit_transaction` relays it via node RPC. Returns the swap deploy hash; the
   * realised output is read separately from an on-chain balance delta (see
   * {@link tokenBalance}) once the deploy is confirmed — `build_swap` only knows the
   * expected amount, never the settled one.
   *
   * The build/submit tool *inputs* are the confirmed CSPR.trade MCP schema; the
   * *return* field names are read defensively (the deploy lives under one of a few
   * documented keys) so a live smoke test can pin them without a code change.
   */
  async swap(params: {
    tokenIn: string;
    tokenOut: string;
    amount: bigint;
    slippageBps: number;
  }): Promise<{ deployHash: string }> {
    const buildTool = this.session.resolveTool(["build_swap"], /^build_swap$/);
    const built = await this.session.call<Record<string, unknown>>(buildTool, {
      token_in: params.tokenIn,
      token_out: params.tokenOut,
      amount: params.amount.toString(),
      type: "exact_in",
      slippage_bps: params.slippageBps,
      sender_public_key: this.senderPublicKeyHex,
    });

    const unsigned =
      built.deploy ??
      built.deploy_json ??
      built.unsigned_deploy ??
      built.transaction ??
      built.tx ??
      built;
    const signedJson = this.signDeployJson(unsigned);

    const submitTool = this.session.resolveTool(["submit_transaction"], /^submit_transaction$/);
    const res = await this.session.call<{
      deploy_hash?: string;
      deployHash?: string;
      transaction_hash?: string;
      transactionHash?: string;
    }>(submitTool, { signed_deploy_json: signedJson });
    const deployHash =
      res.deploy_hash ?? res.deployHash ?? res.transaction_hash ?? res.transactionHash;
    if (!deployHash) {
      throw new Error("CSPR.trade submit_transaction did not return a deploy/transaction hash");
    }
    return { deployHash };
  }

  /**
   * Sign the unsigned deploy `build_swap` returned with the agent key and return
   * the signed-deploy JSON string `submit_transaction` expects. Handles both the
   * legacy `Deploy` shape and the newer `Transaction` shape.
   */
  private signDeployJson(unsigned: unknown): string {
    const json = typeof unsigned === "string" ? JSON.parse(unsigned) : unsigned;
    try {
      const deploy = Deploy.fromJSON(json);
      deploy.sign(this.key);
      return JSON.stringify(Deploy.toJSON(deploy));
    } catch {
      const tx = Transaction.fromJSON(json);
      tx.sign(this.key);
      return JSON.stringify(tx.toJSON());
    }
  }

  /**
   * Read `account`'s balance of `token` (a CEP-18 contract identifier, or "CSPR"
   * for the native token). Used to measure a swap's realised output as a settled
   * balance delta. Return field is read defensively across the documented shapes.
   */
  async tokenBalance(account: string, token: string): Promise<bigint> {
    const isNative = token.toUpperCase() === "CSPR";
    const tool = isNative
      ? this.session.resolveTool(["get_native_cspr_balance"], /^get_native_cspr_balance$/)
      : this.session.resolveTool(["get_token_balance"], /^get_token_balance$/);
    const raw = await this.session.call<Record<string, unknown>>(
      tool,
      isNative ? { account } : { account, token },
    );
    const v = raw.balance ?? raw.amount ?? raw.value ?? raw.motes;
    if (v === undefined) {
      throw new Error(`CSPR.trade ${tool} did not include a balance field`);
    }
    return BigInt(typeof v === "number" ? Math.trunc(v).toString() : (v as string));
  }

  async close(): Promise<void> {
    await this.session.close();
  }
}
