-- R2 non_allowlisted_holder
-- A wallet outside the mint's allowlist holds a positive balance, or received tokens in any transfer.
-- The transfer half catches pass-through: tokens routed via a non-allowlisted wallet and moved on.
SELECT b.mint, b.owner, b.amount, b.slot, CAST(NULL AS VARCHAR) AS signature
FROM balances b
LEFT JOIN allowlist a ON b.mint = a.mint AND b.owner = a.owner
WHERE b.amount > 0 AND a.owner IS NULL
UNION ALL
SELECT t.mint, t.to_owner AS owner, t.amount, t.slot, t.signature
FROM transfers t
LEFT JOIN allowlist a ON t.mint = a.mint AND t.to_owner = a.owner
WHERE t.amount > 0 AND a.owner IS NULL
ORDER BY mint, owner, slot, signature NULLS FIRST
