#!/usr/bin/env python3
"""Save real devnet RPC responses as rc-ingest test fixtures (fixtures/rpc/).

Reads FX_MINT from .secrets/fixtures.env (scripts/fixture_txs.sh) and MINT from .secrets/demo.env
(scripts/devnet_setup.sh). Every request uses commitment=finalized. Each transaction is saved under
a name made of the Token-2022 instruction types it contains, e.g. tx_burnChecked.json.

Usage: python3 -I scripts/capture_fixtures.py
"""
import json
import pathlib
import sys
import time
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "fixtures" / "rpc"
TOKEN_2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
FINALIZED = {"commitment": "finalized"}


def env(name):
    vals = {}
    for line in (ROOT / ".secrets" / name).read_text().splitlines():
        k, _, v = line.partition("=")
        vals[k] = v
    return vals


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(url, body, {"Content-Type": "application/json"})
    for attempt in range(7):
        time.sleep(0.4)  # public devnet RPC rate-limits bursts (HTTP 429)
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                resp = json.load(r)
            if "error" in resp:
                raise RuntimeError(f"{method}: {resp['error']}")
            return resp
        except urllib.error.HTTPError as e:
            if e.code != 429 or attempt == 6:
                raise
            time.sleep(2 ** attempt)
    raise RuntimeError("unreachable")


def ix_types(tx):
    msg = tx["transaction"]["message"]
    inner = [i for group in (tx["meta"].get("innerInstructions") or []) for i in group["instructions"]]
    types = []
    for ix in msg["instructions"] + inner:
        if ix.get("programId") == TOKEN_2022 and isinstance(ix.get("parsed"), dict):
            types.append(ix["parsed"]["type"])
    return types


def save(name, obj):
    path = OUT / f"{name}.json"
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n")
    print(f"  {path.relative_to(ROOT)}")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    demo, fx = env("demo.env"), env("fixtures.env")
    url, fx_mint, demo_mint = demo["RPC"], fx["FX_MINT"], demo["MINT"]

    print(f"waiting for finalized signatures of {fx_mint}")
    for _ in range(30):
        sigs = rpc(url, "getSignaturesForAddress", [fx_mint, {**FINALIZED, "limit": 1000}])
        if len(sigs["result"]) >= 9:
            break
        time.sleep(3)
    save("fx_signatures", sigs)

    seen = {}
    for s in reversed(sigs["result"]):
        tx = rpc(url, "getTransaction", [s["signature"], {**FINALIZED, "encoding": "jsonParsed",
                                                          "maxSupportedTransactionVersion": 0}])
        types = ix_types(tx["result"]) or ["none"]
        name = "tx_" + "+".join(dict.fromkeys(types))
        if name in seen:
            seen[name] += 1
            name = f"{name}_{seen[name]}"
        else:
            seen[name] = 1
        save(name, tx)

    save("fx_token_supply", rpc(url, "getTokenSupply", [fx_mint, FINALIZED]))
    save("fx_mint_account", rpc(url, "getAccountInfo", [fx_mint, {**FINALIZED, "encoding": "jsonParsed"}]))
    save("demo_mint_account", rpc(url, "getAccountInfo", [demo_mint, {**FINALIZED, "encoding": "jsonParsed"}]))
    # Public devnet RPC rejects getProgramAccounts for Token-2022 and throttles
    # getTokenLargestAccounts, so rc-ingest derives balances from meta.postTokenBalances.
    save("demo_token_supply", rpc(url, "getTokenSupply", [demo_mint, FINALIZED]))


if __name__ == "__main__":
    sys.exit(main())
