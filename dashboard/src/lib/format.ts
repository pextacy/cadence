import { PRICE_SCALE, assetDecimals } from "@cadence/mandate";

/** Format a base-unit amount with the asset's decimals (shared source of truth). */
export function formatAmount(value: bigint, asset: string): string {
  const decimals = assetDecimals(asset) || 6;
  const scale = 10n ** BigInt(decimals);
  const whole = value / scale;
  const frac = value % scale;
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  if (frac === 0n) return grouped;
  const fracStr = frac.toString().padStart(decimals, "0").replace(/0+$/, "");
  return `${grouped}.${fracStr}`;
}

/** Format a fixed-point price (PRICE_SCALE) to a decimal string. */
export function formatPrice(price: bigint): string {
  const whole = price / PRICE_SCALE;
  const frac = price % PRICE_SCALE;
  const fracStr = frac.toString().padStart(9, "0").replace(/0+$/, "");
  return fracStr ? `${whole}.${fracStr}` : whole.toString();
}

/** Format a millisecond duration as e.g. "2d 04h" or "03:12:45". */
export function formatDuration(ms: number): string {
  if (ms <= 0) return "0s";
  const totalSec = Math.floor(ms / 1000);
  const days = Math.floor(totalSec / 86_400);
  const hours = Math.floor((totalSec % 86_400) / 3_600);
  const mins = Math.floor((totalSec % 3_600) / 60);
  const secs = totalSec % 60;
  if (days > 0) return `${days}d ${hours.toString().padStart(2, "0")}h`;
  return [hours, mins, secs].map((n) => n.toString().padStart(2, "0")).join(":");
}

/** Format bps as a percentage string, e.g. 150 -> "1.50%". */
export function formatBps(bps: number): string {
  return `${(bps / 100).toFixed(2)}%`;
}

/** Shorten a 0x/hash string for display. */
export function shortHash(hash: string): string {
  if (hash.length <= 16) return hash;
  return `${hash.slice(0, 10)}…${hash.slice(-6)}`;
}
