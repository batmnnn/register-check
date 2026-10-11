-- R1 unbacked_mint
-- A mint whose resulting supply exceeds the latest attested reserve for that mint.
SELECT m.mint, m.signature, m.slot, m.supply_after, r.reserve_amount, r.as_of_ts, r.attestor
FROM mints m
JOIN latest_reserve r ON m.mint = r.mint
WHERE m.supply_after > r.reserve_amount
ORDER BY m.mint, m.slot, m.tx_index, m.ix_index
