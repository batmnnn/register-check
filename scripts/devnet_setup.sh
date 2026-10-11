#!/usr/bin/env bash
# Create the demo Token-2022 mint on devnet: 6 decimals, permanent delegate + transfer-hook extensions
# (hook enabled but unset), holders A-E with 200 tokens each and holder F with an empty account.
# Total supply 1,000 = the demo reserve. All keys go to .secrets/ (gitignored). Safe to re-run: it
# reuses existing keys and skips the mint if .secrets/demo.env exists. FRESH=1 archives the current
# demo and creates a new mint (for another all-green take; history on the old mint is permanent).
set -euo pipefail
cd "$(dirname "$0")/.."

SECRETS=.secrets
PAYER=${PAYER:-$HOME/.config/solana/id.json}
RPC=${RPC:-https://api.devnet.solana.com}
mkdir -p "$SECRETS"
chmod 700 "$SECRETS"

CFG=$SECRETS/solana-cli.yml
cat > "$CFG" <<YML
json_rpc_url: "$RPC"
websocket_url: ""
keypair_path: "$PAYER"
commitment: confirmed
YML
SOL() { solana --config "$CFG" "$@"; }
SPL() { spl-token --config "$CFG" --program-2022 "$@"; }

key() { # key <name> -> path, creating the keypair if missing
  local f="$SECRETS/$1.json"
  [ -f "$f" ] || solana-keygen new --no-bip39-passphrase --silent -o "$f" >/dev/null
  echo "$f"
}
pub() { solana-keygen pubkey "$(key "$1")"; }

MINT_KEY=demo-mint
if [ -f "$SECRETS/demo.env" ]; then
  if [ "${FRESH:-0}" != 1 ]; then
    echo "demo mint already set up (FRESH=1 for a new one):"; cat "$SECRETS/demo.env"; exit 0
  fi
  ts=$(date +%s)
  mv "$SECRETS/demo.env" "$SECRETS/demo.env.$ts"
  MINT_KEY="demo-mint-$ts"
  echo "archived previous demo to $SECRETS/demo.env.$ts"
fi

echo "payer $(SOL address) balance $(SOL balance)"
MINT_KP=$(key "$MINT_KEY")
MINT=$(solana-keygen pubkey "$MINT_KP")

SPL create-token "$MINT_KP" --decimals 6 --enable-permanent-delegate --enable-transfer-hook >/dev/null
echo "mint $MINT created"

for h in A B C D E F; do
  owner=$(pub "holder-$h")
  SPL create-account "$MINT" --owner "$owner" --fee-payer "$PAYER" >/dev/null
  if [ "$h" != F ]; then
    SPL mint "$MINT" 200 --recipient-owner "$owner" >/dev/null
  fi
  echo "holder $h $owner"
done

{
  echo "MINT=$MINT"
  echo "RPC=$RPC"
  for h in A B C D E F; do echo "HOLDER_$h=$(pub "holder-$h")"; done
  echo "ATTESTOR=$(pub attestor)"
  echo "HOOK_A=$(pub hook-a)"
} > "$SECRETS/demo.env"

echo "supply $(SPL supply "$MINT")"
echo "wrote $SECRETS/demo.env"
