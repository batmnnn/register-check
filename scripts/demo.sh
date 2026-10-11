#!/usr/bin/env bash
# Register Check demo driver (devnet). Run scripts/devnet_setup.sh first.
#   demo.sh init            allowlist A-F, register the attestor, sign a 1,000 reserve
#   demo.sh watch           run the watcher (proofs/ + proofs/status.json)
#   demo.sh mint <n>        mint n tokens to holder F (n=100 pushes supply over the reserve -> R1)
#   demo.sh swap-hook       point the transfer hook at a new program (-> R4). Transfers then fail
#                           until unhook: the demo hook address is not a deployed program
#   demo.sh unhook          set the transfer hook back to none (-> second R4 row)
#   demo.sh reserve <n>     issuer signs a new reserve attestation of n tokens
#   demo.sh stranger <n>    holder A sends n tokens to a wallet not on the allowlist (-> R2)
set -euo pipefail
cd "$(dirname "$0")/.."
SECRETS=.secrets RUN=run
[ -f "$SECRETS/demo.env" ] || { echo "run scripts/devnet_setup.sh first"; exit 1; }
# shellcheck disable=SC1091
source "$SECRETS/demo.env"
CFG=$SECRETS/solana-cli.yml
PAYER=${PAYER:-$HOME/.config/solana/id.json}
SPL() { spl-token --config "$CFG" --program-2022 "$@"; }
key() { local f="$SECRETS/$1.json"; [ -f "$f" ] || solana-keygen new --no-bip39-passphrase --silent -o "$f" >/dev/null; echo "$f"; }
RC=target/debug/rc
build() { [ -x "$RC" ] && [ -z "$(find crates rules -newer "$RC" -name '*.rs' -o -newer "$RC" -name '*.sql' 2>/dev/null | head -1)" ] || cargo build -q -p rc-cli; }

cmd=${1:-}; shift || true
case "$cmd" in
  init)
    build; mkdir -p "$RUN"
    echo "[\"$ATTESTOR\"]" > "$RUN/attestors.json"
    { echo "["; first=1; for h in A B C D E F; do v="HOLDER_$h"; [ $first = 1 ] || echo ","; first=0
        printf '  {"mint": "%s", "owner": "%s"}' "$MINT" "${!v}"; done; echo; echo "]"; } > "$RUN/allowlist.json"
    rm -f "$RUN/reserves.json"
    $RC sign-reserve --keypair "$(key attestor)" --mint "$MINT" --amount "${1:-1000}" --decimals 6 \
      --source-url "https://example.invalid/rusd/reserve-report.json" --out "$RUN/reserves.json"
    echo "init done: $RUN/{attestors,allowlist,reserves}.json" ;;
  watch)
    build
    exec $RC watch --mint "$MINT" --rpc "$RPC" --reserves "$RUN/reserves.json" \
      --attestors "$RUN/attestors.json" --allowlist "$RUN/allowlist.json" --out proofs "$@" ;;
  mint)
    n=${1:?usage: demo.sh mint <n>}
    SPL mint "$MINT" "$n" --recipient-owner "$HOLDER_F" | grep -E 'Signature|Minting' ;;
  swap-hook)
    SPL set-transfer-hook "$MINT" "$(solana-keygen pubkey "$(key hook-b)")" | grep Signature ;;
  unhook)
    SPL set-transfer-hook "$MINT" --disable | grep Signature ;;
  reserve)
    build; n=${1:?usage: demo.sh reserve <n>}
    $RC sign-reserve --keypair "$(key attestor)" --mint "$MINT" --amount "$n" --decimals 6 \
      --source-url "https://example.invalid/rusd/reserve-report.json" --out "$RUN/reserves.json" ;;
  stranger)
    n=${1:?usage: demo.sh stranger <n>}
    SPL transfer "$MINT" "$n" "$(solana-keygen pubkey "$(key stranger)")" --owner "$(key holder-A)" \
      --fee-payer "$PAYER" --fund-recipient --allow-unfunded-recipient | grep Signature ;;
  *) sed -n '2,11p' "$0"; exit 1 ;;
esac
