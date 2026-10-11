//! Register Check attestation program. Stores what was checked (rule id + sha256 of the exact
//! SQL) and what the check found (result hash of the canonical proof bundle). No token logic:
//! anyone can re-fetch the finalized tx, re-run the SQL, and compare hashes.

use anchor_lang::prelude::*;

declare_id!("BNADNdmc5zJF54Tj6o1ctr3PmhjjDFPByPGGU3oLthV3");

pub const CONFIG_SEED: &[u8] = b"config";
pub const RULE_SEED: &[u8] = b"rule";
pub const RESULT_SEED: &[u8] = b"result";

#[program]
pub mod rc_attest {
    use super::*;

    /// One-time setup; the payer becomes the authority allowed to register rules and post results.
    pub fn init_config(ctx: Context<InitConfig>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.authority = ctx.accounts.authority.key();
        config.bump = ctx.bumps.config;
        Ok(())
    }

    /// Register a rule version. A changed SQL file has a new `sql_hash` and gets its own account,
    /// so every result stays tied to the exact SQL that produced it.
    pub fn register_rule(
        ctx: Context<RegisterRule>,
        id: [u8; 32],
        sql_hash: [u8; 32],
    ) -> Result<()> {
        require!(id != [0u8; 32], AttestError::EmptyRuleId);
        let rule = &mut ctx.accounts.rule;
        rule.id = id;
        rule.sql_hash = sql_hash;
        rule.bump = ctx.bumps.rule;
        emit!(RuleRegistered {
            rule: rule.key(),
            id,
            sql_hash
        });
        Ok(())
    }

    /// Record one check result. Seeded by `result_hash`, so the same evidence can't be posted twice
    /// and two results in one slot never collide.
    pub fn post_result(
        ctx: Context<PostResult>,
        slot: u64,
        result_hash: [u8; 32],
        violated: bool,
    ) -> Result<()> {
        require!(result_hash != [0u8; 32], AttestError::EmptyResultHash);
        let posted_at = Clock::get()?.unix_timestamp;
        let result = &mut ctx.accounts.result;
        result.rule = ctx.accounts.rule.key();
        result.slot = slot;
        result.result_hash = result_hash;
        result.violated = violated;
        result.posted_at = posted_at;
        result.bump = ctx.bumps.result;
        emit!(ResultPosted {
            result: result.key(),
            rule: result.rule,
            slot,
            result_hash,
            violated,
            posted_at
        });
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub authority: Pubkey,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Rule {
    /// Rule id, UTF-8 zero-padded (e.g. "R1_unbacked_mint").
    pub id: [u8; 32],
    /// sha256 of the rule's .sql file bytes.
    pub sql_hash: [u8; 32],
    pub bump: u8,
}

/// Named CheckResult because `Result` would shadow `anchor_lang::Result`.
#[account]
#[derive(InitSpace)]
pub struct CheckResult {
    pub rule: Pubkey,
    /// Slot of the evidence (finalized).
    pub slot: u64,
    /// sha256 of the canonical proof bundle.
    pub result_hash: [u8; 32],
    pub violated: bool,
    pub posted_at: i64,
    pub bump: u8,
}

#[derive(Accounts)]
pub struct InitConfig<'info> {
    #[account(init, payer = authority, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(id: [u8; 32], sql_hash: [u8; 32])]
pub struct RegisterRule<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = authority @ AttestError::Unauthorized)]
    pub config: Account<'info, Config>,
    #[account(
        init,
        payer = authority,
        space = 8 + Rule::INIT_SPACE,
        seeds = [RULE_SEED, id.as_ref(), sql_hash.as_ref()],
        bump
    )]
    pub rule: Account<'info, Rule>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(slot: u64, result_hash: [u8; 32])]
pub struct PostResult<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = authority @ AttestError::Unauthorized)]
    pub config: Account<'info, Config>,
    #[account(seeds = [RULE_SEED, rule.id.as_ref(), rule.sql_hash.as_ref()], bump = rule.bump)]
    pub rule: Account<'info, Rule>,
    #[account(
        init,
        payer = authority,
        space = 8 + CheckResult::INIT_SPACE,
        seeds = [RESULT_SEED, rule.key().as_ref(), result_hash.as_ref()],
        bump
    )]
    pub result: Account<'info, CheckResult>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[event]
pub struct RuleRegistered {
    pub rule: Pubkey,
    pub id: [u8; 32],
    pub sql_hash: [u8; 32],
}

#[event]
pub struct ResultPosted {
    pub result: Pubkey,
    pub rule: Pubkey,
    pub slot: u64,
    pub result_hash: [u8; 32],
    pub violated: bool,
    pub posted_at: i64,
}

#[error_code]
pub enum AttestError {
    #[msg("signer is not the config authority")]
    Unauthorized,
    #[msg("rule id must not be empty")]
    EmptyRuleId,
    #[msg("result hash must not be all zeros")]
    EmptyResultHash,
}
