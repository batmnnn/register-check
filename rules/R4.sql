-- R4 privileged_change
-- Mint/freeze authority, permanent delegate, transfer-hook program or its authority changed.
SELECT mint, signature, slot, change_kind, old_value, new_value
FROM authority_changes
WHERE change_kind IN ('mint_authority', 'freeze_authority', 'permanent_delegate', 'transfer_hook_program', 'transfer_hook_authority')
ORDER BY mint, slot, tx_index, ix_index
