import casper from "casper-js-sdk";
import type * as Casper from "casper-js-sdk";
import { RpcConfirmationService, type ConfirmationService, type ConfirmOptions } from "./confirm.js";
import type { VaultStateReader } from "../state/reconcile.js";

// casper-js-sdk is CommonJS; its API is on the default export. Destructure values
// from default and take type-only names from the namespace import.
const { Args, CLValue, ContractCallBuilder, HttpHandler, KeyAlgorithm, PrivateKey, RpcClient } =
  casper;

/** Default gas payment (motes) for the agent's vault entrypoints. */
const GAS_EXECUTE_SLICE = 8_000_000_000;
const GAS_RECORD_FILL = 3_000_000_000;
const GAS_ATTEST = 2_000_000_000;
const GAS_SETTLE = 5_000_000_000;
const GAS_PAUSE = 1_500_000_000;
const GAS_FLUSH_FEES = 3_000_000_000;

export interface VaultClientOptions {
  nodeRpcUrl: string;
  chainName: string;
  contractHash: string;
  /** Agent secp256k1 private key, hex (with or without 0x). */
  agentPrivateKeyHex: string;
}

/**
 * Submits the agent's constrained vault entrypoints as signed Casper transactions.
 * The agent identity is the only caller the vault authorises for these calls; the
 * vault re-validates every guardrail on-chain regardless of what is submitted.
 */
export class VaultClient {
  private readonly rpc: Casper.RpcClient;
  private readonly key: Casper.PrivateKey;
  private readonly contractHash: string;
  private readonly chainName: string;

  constructor(opts: VaultClientOptions) {
    this.rpc = new RpcClient(new HttpHandler(opts.nodeRpcUrl));
    this.key = PrivateKey.fromHex(
      opts.agentPrivateKeyHex.replace(/^0x/, ""),
      KeyAlgorithm.SECP256K1,
    );
    this.contractHash = opts.contractHash;
    this.chainName = opts.chainName;
  }

  /** The agent's on-chain account-hash identity ("account-hash-…"). */
  agentAccountHash(): string {
    return this.key.publicKey.accountHash().toPrefixedString();
  }

  /**
   * A {@link ConfirmationService} bound to this client's RPC connection, so the
   * executor can poll the vault entrypoint transactions and the venue swap deploy
   * to finality over the same node without opening a second connection.
   */
  confirmationService(defaults?: ConfirmOptions): ConfirmationService {
    return new RpcConfirmationService(this.rpc, defaults ?? {});
  }

  /**
   * This client's RPC connection narrowed to the read primitives on-chain
   * reconciliation needs ({@link VaultStateReader}). `RpcClient` structurally
   * implements both methods, so the executor/loop can seed authoritative vault
   * state from chain without opening a second connection.
   */
  stateReader(): VaultStateReader {
    return this.rpc as unknown as VaultStateReader;
  }

  private async send(entryPoint: string, args: Casper.Args, gasMotes: number): Promise<string> {
    const tx = new ContractCallBuilder()
      .from(this.key.publicKey)
      .byHash(this.contractHash)
      .entryPoint(entryPoint)
      .runtimeArgs(args)
      .chainName(this.chainName)
      .payment(gasMotes)
      .build();
    tx.sign(this.key);
    const result = await this.rpc.putTransaction(tx);
    return result.transactionHash.toJSON();
  }

  /** Build (without sending) — exposed for testing/inspection. */
  buildExecuteSlice(p: {
    sellAmount: bigint;
    quotedOut: bigint;
    minOut: bigint;
    venue: string;
  }): Casper.Transaction {
    return new ContractCallBuilder()
      .from(this.key.publicKey)
      .byHash(this.contractHash)
      .entryPoint("execute_slice")
      .runtimeArgs(this.executeSliceArgs(p))
      .chainName(this.chainName)
      .payment(GAS_EXECUTE_SLICE)
      .build();
  }

  private executeSliceArgs(p: {
    sellAmount: bigint;
    quotedOut: bigint;
    minOut: bigint;
    venue: string;
  }): Casper.Args {
    // No venue address is sent: the vault resolves the destination from the
    // mandate-bound allowlist on-chain, so the agent cannot redirect funds.
    return Args.fromMap({
      sell_amount: CLValue.newCLUInt512(p.sellAmount.toString()),
      quoted_out: CLValue.newCLUInt512(p.quotedOut.toString()),
      min_out: CLValue.newCLUInt512(p.minOut.toString()),
      venue: CLValue.newCLString(p.venue),
    });
  }

  async executeSlice(p: {
    sellAmount: bigint;
    quotedOut: bigint;
    minOut: bigint;
    venue: string;
  }): Promise<string> {
    return this.send("execute_slice", this.executeSliceArgs(p), GAS_EXECUTE_SLICE);
  }

  async recordFill(p: {
    sliceId: number;
    boughtAmount: bigint;
    swapDeployHash: string;
  }): Promise<string> {
    const args = Args.fromMap({
      slice_id: CLValue.newCLUInt32(p.sliceId),
      bought_amount: CLValue.newCLUInt512(p.boughtAmount.toString()),
      swap_deploy_hash: CLValue.newCLString(p.swapDeployHash),
    });
    return this.send("record_fill", args, GAS_RECORD_FILL);
  }

  async attest(p: { sliceId: number; reason: string }): Promise<string> {
    const args = Args.fromMap({
      slice_id: CLValue.newCLUInt32(p.sliceId),
      reason: CLValue.newCLString(p.reason),
    });
    return this.send("attest", args, GAS_ATTEST);
  }

  async pause(): Promise<string> {
    return this.send("pause", Args.fromMap({}), GAS_PAUSE);
  }

  async resume(): Promise<string> {
    return this.send("resume", Args.fromMap({}), GAS_PAUSE);
  }

  async settle(): Promise<string> {
    return this.send("settle", Args.fromMap({}), GAS_SETTLE);
  }

  /**
   * Push the locally accumulated protocol-fee base to the wired fee module.
   * Recorded fills only accrue a fee obligation in-vault; this decoupled
   * entrypoint performs the actual cross-contract fee push. It is agent- or
   * treasury-callable and reverts benignly when there is nothing to do:
   * `FeeNotActive` (error 25) if no fee module is wired, `NothingToFlush`
   * (error 26) if nothing is accumulated. Callers treat those as no-ops.
   */
  async flushFees(): Promise<string> {
    return this.send("flush_fees", Args.fromMap({}), GAS_FLUSH_FEES);
  }
}
