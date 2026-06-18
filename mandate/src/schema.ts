import {
  buildDomain,
  type EIP712Domain,
  type TypeDefinitions,
} from "@casper-ecosystem/casper-eip-712";

/**
 * Fixed-point scale for prices (buy-asset units per one sell-asset unit), matching
 * `PRICE_SCALE` in the Execution Vault contract. A price of 1.0 is `PRICE_SCALE`.
 */
export const PRICE_SCALE = 1_000_000_000n;

/** Basis-points denominator (100% = 10_000 bps), matching the contract. */
export const BPS_DENOMINATOR = 10_000;

/**
 * Base-unit decimals per asset symbol. The single source of truth for converting
 * between human and base-unit amounts, shared by the signer and the UI so they can
 * never disagree. Unknown symbols are treated as already being in base units (0
 * decimals) rather than guessing.
 */
export const ASSET_DECIMALS: Record<string, number> = {
  CSPR: 9,
  USDC: 6,
  USDT: 6,
};

/** Decimals for an asset symbol; 0 (i.e. already base units) when unknown. */
export function assetDecimals(asset: string): number {
  return ASSET_DECIMALS[asset.toUpperCase()] ?? 0;
}

/** Convert a human-entered decimal amount of `asset` into integer base units. */
export function toBaseUnits(human: string, asset: string): bigint {
  const decimals = assetDecimals(asset);
  const [whole, frac = ""] = human.trim().split(".");
  const fracPadded = (frac + "0".repeat(decimals)).slice(0, decimals);
  return BigInt((whole || "0") + fracPadded);
}

export type Strategy = "TWAP" | "VWAP";

/**
 * A mandate is the single artefact the treasurer signs. Amounts are decimal
 * strings in the asset's base units (e.g. motes for CSPR). Prices are decimal
 * strings in fixed-point with {@link PRICE_SCALE}; "0" means unset.
 *
 * `treasury` is the 0x-prefixed 20-byte secp256k1 signer address that authorises
 * the mandate (EIP-712). The on-chain Casper account that funds the vault is the
 * transaction sender and is bound separately by the vault's `init` caller.
 */
export interface Mandate {
  version: number;
  treasury: string;
  sellAsset: string;
  buyAsset: string;
  totalSellAmount: string;
  startTime: number;
  endTime: number;
  maxSlippageBps: number;
  priceFloor: string;
  priceCeiling: string;
  strategy: Strategy;
  venueAllowlist: string[];
  nonce: string;
}

/** The EIP-712 primary type name for a mandate. */
export const MANDATE_PRIMARY_TYPE = "Mandate";

/**
 * EIP-712 type definition for a mandate. The venue allowlist is encoded as a
 * single canonical comma-joined `string` (`venues`) so the hash is deterministic
 * without relying on dynamic-array encoding; the vault stores the set itself.
 */
export const MANDATE_TYPES: TypeDefinitions = {
  Mandate: [
    { name: "version", type: "uint256" },
    { name: "treasury", type: "address" },
    { name: "sellAsset", type: "string" },
    { name: "buyAsset", type: "string" },
    { name: "totalSellAmount", type: "uint256" },
    { name: "startTime", type: "uint256" },
    { name: "endTime", type: "uint256" },
    { name: "maxSlippageBps", type: "uint256" },
    { name: "priceFloor", type: "uint256" },
    { name: "priceCeiling", type: "uint256" },
    { name: "strategy", type: "string" },
    { name: "venues", type: "string" },
    { name: "nonce", type: "bytes32" },
  ],
};

/** Domain name and version for all Cadence mandates. */
export const DOMAIN_NAME = "Cadence";
export const DOMAIN_VERSION = "1";

/**
 * EIP-712 `verifyingContract` for the mandate domain. The mandate is signed
 * **before** the vault exists (the vault's `init` consumes the signed digest), so
 * the domain cannot bind to a specific vault package hash without an impossible
 * chicken-and-egg. Instead the domain is chain-scoped: replay protection comes
 * from the per-mandate `nonce`, and the mandate↔vault binding runs the other way —
 * the vault stores and emits the digest on `init`, committing itself to one
 * mandate. A verifier re-derives the digest from the public mandate + this domain
 * and checks the on-chain stored digest matches.
 */
export const MANDATE_DOMAIN_VERIFIER = `0x${"00".repeat(32)}`;

/**
 * Build the chain-scoped EIP-712 domain for a mandate.
 *
 * @param chainName Casper chain name, e.g. "casper-test".
 */
export function buildMandateDomain(chainName: string): EIP712Domain {
  return buildDomain(
    DOMAIN_NAME,
    DOMAIN_VERSION,
    `casper:${chainName}`,
    MANDATE_DOMAIN_VERIFIER,
  );
}

/** Canonical comma-joined venue string used in the typed data. */
export function canonicalVenues(venueAllowlist: string[]): string {
  return venueAllowlist.join(",");
}

/**
 * Build the EIP-712 message object (field name → encodable value) from a mandate.
 * Numeric base-unit fields are converted to bigint so the encoder treats them as
 * decimal integers rather than hex.
 */
export function toTypedMessage(m: Mandate): Record<string, unknown> {
  return {
    version: BigInt(m.version),
    treasury: m.treasury,
    sellAsset: m.sellAsset,
    buyAsset: m.buyAsset,
    totalSellAmount: BigInt(m.totalSellAmount),
    startTime: BigInt(m.startTime),
    endTime: BigInt(m.endTime),
    maxSlippageBps: BigInt(m.maxSlippageBps),
    priceFloor: BigInt(m.priceFloor),
    priceCeiling: BigInt(m.priceCeiling),
    strategy: m.strategy,
    venues: canonicalVenues(m.venueAllowlist),
    nonce: m.nonce,
  };
}

/** Render a mandate as a single plain-language sentence for the UI. */
export function humanSummary(m: Mandate): string {
  const amount = formatBaseUnits(m.totalSellAmount, m.sellAsset);
  const deadline = new Date(m.endTime * 1000).toUTCString();
  const slippage = (m.maxSlippageBps / 100).toFixed(2);
  const band = priceBandClause(m);
  return (
    `Sell ${amount} for ${m.buyAsset} by ${deadline} using ${m.strategy}, ` +
    `never worse than ${slippage}% slippage${band}.`
  );
}

function priceBandClause(m: Mandate): string {
  const floor = BigInt(m.priceFloor);
  const ceiling = BigInt(m.priceCeiling);
  if (floor === 0n && ceiling === 0n) return "";
  const parts: string[] = [];
  if (floor > 0n) parts.push(`floor ${fixedToDecimal(floor)}`);
  if (ceiling > 0n) parts.push(`ceiling ${fixedToDecimal(ceiling)}`);
  return `, price band ${parts.join(" / ")} ${m.buyAsset}/${m.sellAsset}`;
}

function fixedToDecimal(fixed: bigint): string {
  const whole = fixed / PRICE_SCALE;
  const frac = fixed % PRICE_SCALE;
  if (frac === 0n) return whole.toString();
  const fracStr = frac.toString().padStart(PRICE_SCALE.toString().length - 1, "0").replace(/0+$/, "");
  return `${whole}.${fracStr}`;
}

/** Human-friendly base-unit formatting; decimals come from {@link assetDecimals}. */
function formatBaseUnits(amount: string, asset: string): string {
  const decimals = assetDecimals(asset);
  if (decimals === 0) return `${amount} ${asset}`;
  const v = BigInt(amount);
  const scale = 10n ** BigInt(decimals);
  const whole = v / scale;
  const frac = v % scale;
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  if (frac === 0n) return `${grouped} ${asset}`;
  const fracStr = frac.toString().padStart(decimals, "0").replace(/0+$/, "");
  return `${grouped}.${fracStr} ${asset}`;
}
