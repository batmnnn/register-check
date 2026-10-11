-- R3 holder_concentration
-- Any holder with more than 20% of current supply. Cast before multiplying: u64 * 10000 overflows.
SELECT b.mint, b.owner, b.amount, s.supply
FROM balances b
JOIN supply s ON b.mint = s.mint
WHERE s.supply > 0
  AND CAST(b.amount AS DECIMAL(38, 0)) * 10000 > CAST(s.supply AS DECIMAL(38, 0)) * 2000
ORDER BY b.mint, b.owner
