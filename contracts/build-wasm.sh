#!/usr/bin/env bash
# Build deployable Casper WASM for every Cadence contract.
#
# Why this script exists: the contracts are a single Cargo workspace, so all
# crates compile into the shared workspace `target/` directory. `cargo odra build`
# (cargo-odra 0.1.7) expects each crate's wasm under that crate's *local* target
# dir and its copy step fails in a workspace. This script reproduces what
# cargo-odra does under the hood and writes the result to `<crate>/wasm/`.
#
# The crucial detail cargo-odra handles (and a plain `cargo build` does not): each
# contract must be compiled with `--cfg odra_module="<Contract>"`, which is what
# makes the Odra macros emit that contract's exported `call` entrypoint. Without
# it the contract logic is dead-code-eliminated and every crate yields the same
# empty wasm. Because the cfg is per-contract, each contract gets its own build.
set -euo pipefail

cd "$(dirname "$0")"
TARGET_DIR="target/wasm32-unknown-unknown/release"

# crate : build-contract bin : module name (the Odra.toml fqn type / wasm name)
CONTRACTS=(
  "vault:cadence_vault_build_contract:ExecutionVault"
  "cep18:cadence_cep18_build_contract:Cep18"
  "x402-token:cadence_x402_token_build_contract:X402Token"
  "access-control:cadence_access_control_build_contract:AccessControlContract"
  "dex-adapter:cadence_dex_adapter_build_contract:SettlementAdapter"
  "dex-adapter:cadence_dex_adapter_build_contract:Cep18SwapAdapter"
  "price-oracle:cadence_price_oracle_build_contract:SignedPriceOracle"
)

for entry in "${CONTRACTS[@]}"; do
  IFS=":" read -r crate bin name <<< "$entry"
  src="$TARGET_DIR/${bin}.wasm"
  dest_dir="$crate/wasm"
  dest="$dest_dir/${name}.wasm"

  echo "==> Compiling $name ($crate)…"
  # Mirror cargo-odra: select the contract via ODRA_MODULE + the odra_module cfg
  # so its `call` entrypoint is emitted. RUSTFLAGS applies to the whole build
  # (the contract lib is compiled with the same cfg as its build-contract bin).
  ODRA_MODULE="$name" \
  RUSTFLAGS="--cfg odra_module=\"$name\" --cfg odra_backend=\"\"" \
    cargo build --release --target wasm32-unknown-unknown --bin "$bin"

  if [[ ! -f "$src" ]]; then
    echo "ERROR: expected wasm not found at $src" >&2
    exit 1
  fi

  mkdir -p "$dest_dir"
  cp "$src" "$dest"

  # Casper runs MVP wasm. The Rust sysroot's precompiled memcpy/memset emit
  # bulk-memory and sign-ext opcodes the engine rejects, so read those proposals
  # and lower them back to MVP (disabling the features in the output), then
  # size-optimise. Skips gracefully if the binaryen toolchain is unavailable.
  if command -v wasm-strip >/dev/null 2>&1; then
    wasm-strip "$dest"
  fi
  if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt "$dest" -o "$dest" \
      --enable-bulk-memory-opt --enable-sign-ext --enable-nontrapping-float-to-int \
      --llvm-memory-copy-fill-lowering --llvm-nontrapping-fptoint-lowering --signext-lowering \
      --strip-debug -Oz
  fi

  printf "    %-14s -> %s (%s)\n" "$crate" "$dest" "$(du -h "$dest" | cut -f1)"
done

echo "==> Done. Deployable wasm written under each crate's wasm/ directory."
