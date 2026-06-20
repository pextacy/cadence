/**
 * Deploy-safety guard. Casper mainnet deploys/funds move real value, so they must
 * never happen by accident: a mainnet target requires an explicit opt-in. This is a
 * pure, side-effect-free check (no env reads, no logging) so it is trivially
 * unit-testable — callers pass the resolved network and the opt-in flag.
 */

/** Network selectors that resolve to Casper mainnet (friendly name + chain name). */
const MAINNET_ALIASES = new Set(["mainnet", "casper"]);

/**
 * Throw unless deploying to the given `network` is allowed.
 *
 * - Testnet (or anything that is not a recognised mainnet alias) is always a no-op.
 * - Mainnet (`mainnet` / `casper`, case-insensitive, surrounding whitespace ignored)
 *   throws unless `allowMainnet` is true.
 *
 * Pure: it inspects only its arguments and either returns `void` or throws.
 *
 * @param network selected network — friendly name or chain name (e.g. "testnet",
 *   "mainnet", "casper", "casper-test").
 * @param allowMainnet explicit opt-in; callers should pass
 *   `process.env.ALLOW_MAINNET === "true"`.
 */
export function assertDeployTargetAllowed(network: string, allowMainnet: boolean): void {
  const normalized = network.trim().toLowerCase();
  if (!MAINNET_ALIASES.has(normalized)) return; // testnet / unknown: no guard.
  if (allowMainnet) return; // opted in.

  throw new Error(
    `Refusing to deploy to Casper mainnet (CASPER_NETWORK="${network}"). ` +
      `Mainnet moves real funds. Set ALLOW_MAINNET=true to proceed on mainnet.`,
  );
}
