#!/usr/bin/env python3
"""Independently verify a Register Check proof bundle's result_hash (no Rust needed).

result_hash = sha256 of the bundle as compact JSON with sorted keys, after removing
`result_hash` and `attested_tx`. Usage: python3 -I scripts/verify_proof.py proofs/*.json
"""
import hashlib
import json
import sys


def result_hash(bundle):
    body = {k: v for k, v in bundle.items() if k not in ("result_hash", "attested_tx")}
    canonical = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def main(paths):
    ok = True
    for p in paths:
        with open(p, encoding="utf-8") as f:
            b = json.load(f)
        got = result_hash(b)
        match = got == b.get("result_hash")
        ok &= match
        print(f"{'OK  ' if match else 'FAIL'} {p} {b.get('rule')} {got}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
