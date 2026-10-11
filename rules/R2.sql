-- R2 non_allowlisted_holder
-- Any owner holding a positive balance who is not on the mint's allowlist.
SELECT b.mint, b.owner, b.amount, b.slot
FROM balances b
LEFT JOIN allowlist a ON b.mint = a.mint AND b.owner = a.owner
WHERE b.amount > 0 AND a.owner IS NULL
ORDER BY b.mint, b.owner
