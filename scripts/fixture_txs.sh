#!/usr/bin/env bash
# Exercise every instruction rc-ingest parses on a throwaway Token-2022 mint (not the demo mint),
# so scripts/capture_fixtures.py can save the real jsonParsed transactions as test fixtures.
set -euo pipefail
cd "$(dirname "$0")/.."
SECRETS=.secrets
CFG=$SECRETS/solana-cli.yml
PAYER=${PAYER:-$HOME/.config/solana/id.json}
[ -f "$CFG" ] || { echo "run scripts/devnet_setup.sh first"; exit 1; }
SPL() { spl-token --config "$CFG" --program-2022 "$@"; }
key() { local f="$SECRETS/$1.json"; [ -f "$f" ] || solana-keygen new --no-bip39-passphrase --silent -o "$f" >/dev/null; echo "$f"; }
pub() { solana-keygen pubkey "$(key "$1")"; }
ata() { SPL address --token "$1" --owner "$2" --verbose | awk '/Associated token address/ {print $4}'; }
step() { echo "== $1"; }

MINT_KP=$(key "${FX_MINT_KEY:-fx-mint}"); MINT=$(solana-keygen pubkey "$MINT_KP")
A=$(pub fx-holder-a); B=$(pub fx-holder-b)

step "create-token $MINT";  SPL create-token "$MINT_KP" --decimals 6 --enable-permanent-delegate --enable-transfer-hook --enable-freeze >/dev/null
step "accounts";            SPL create-account "$MINT" --owner "$A" --fee-payer "$PAYER" >/dev/null
                            SPL create-account "$MINT" --owner "$B" --fee-payer "$PAYER" >/dev/null
step "mint 500 -> A";       SPL mint "$MINT" 500 --recipient-owner "$A" >/dev/null
step "transfer 50 A -> B";  SPL transfer "$MINT" 50 "$B" --owner "$(key fx-holder-a)" --fee-payer "$PAYER" --allow-unfunded-recipient >/dev/null
step "permanent-delegate burn 30 from B"
                            SPL burn "$(ata "$MINT" "$B")" 30 --owner "$PAYER" >/dev/null
step "owner burn 20 from A"; SPL burn "$(ata "$MINT" "$A")" 20 --owner "$(key fx-holder-a)" --fee-payer "$PAYER" >/dev/null
step "set transfer hook";   SPL set-transfer-hook "$MINT" "$(pub hook-a)" >/dev/null
step "freeze authority";    SPL authorize "$MINT" freeze "$(pub fx-new-freeze)" >/dev/null
step "mint authority";      SPL authorize "$MINT" mint "$(pub fx-new-mint-auth)" >/dev/null
step "permanent delegate"; SPL authorize "$MINT" permanent-delegate "$(pub fx-new-pdelegate)" >/dev/null
echo "FX_MINT=$MINT" > "$SECRETS/fixtures.env"
echo "supply $(SPL supply "$MINT")"
