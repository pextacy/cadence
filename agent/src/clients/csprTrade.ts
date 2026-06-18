import { McpSession } from "./mcp.js";
import type { Quote } from "../types.js";

/** Raw quote fields the CSPR.trade MCP may return; mapped into {@link Quote}. */
interface RawQuote {
  amount_out?: string | number;
  amountOut?: string | number;
  min_received?: string | number;
  minReceived?: string | number;
  out_amount?: string | number;
  venue_address?: string;
  venueAddress?: string;
  pool?: string;
  route_id?: string;
  routeId?: string;
}

function pickAmount(q: RawQuote): bigint {
  const v = q.amount_out ?? q.amountOut ?? q.out_amount ?? q.min_received ?? q.minReceived;
  if (v === undefined) {
    throw new Error("CSPR.trade quote did not include an output amount field");
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

  constructor(
    private readonly url: string,
    private readonly venue: string,
  ) {
    this.session = new McpSession(url, undefined, "cadence-cspr-trade");
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
    const tool = this.session.resolveTool(["get_quote", "quote"], /quote/i);
    const raw = await this.session.call<RawQuote>(tool, {
      token_in: params.tokenIn,
      token_out: params.tokenOut,
      amount: params.amount.toString(),
      type: "exact_in",
      ...(params.venue ? { venue: params.venue } : {}),
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
   * Submit a swap for a quoted route. Returns the swap deploy/transaction hash so
   * the executor can link it on-chain in `record_fill`. The exact tool arguments
   * follow the server schema discovered at connect time.
   */
  async executeSwap(params: {
    tokenIn: string;
    tokenOut: string;
    amount: bigint;
    minOut: bigint;
    recipient: string;
    routeId?: string;
  }): Promise<{ deployHash: string; boughtAmount: bigint }> {
    const tool = this.session.resolveTool(["execute_swap", "swap"], /swap|execute/i);
    const raw = await this.session.call<{
      deploy_hash?: string;
      deployHash?: string;
      transaction_hash?: string;
      amount_out?: string | number;
      amountOut?: string | number;
    }>(tool, {
      token_in: params.tokenIn,
      token_out: params.tokenOut,
      amount: params.amount.toString(),
      min_out: params.minOut.toString(),
      recipient: params.recipient,
      ...(params.routeId ? { route_id: params.routeId } : {}),
    });
    const deployHash = raw.deploy_hash ?? raw.deployHash ?? raw.transaction_hash;
    const out = raw.amount_out ?? raw.amountOut;
    if (!deployHash || out === undefined) {
      throw new Error("CSPR.trade swap did not return a deploy hash and output amount");
    }
    return {
      deployHash,
      boughtAmount: BigInt(typeof out === "number" ? Math.trunc(out).toString() : out),
    };
  }

  async close(): Promise<void> {
    await this.session.close();
  }
}
