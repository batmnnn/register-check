//! LiteSVM tests for rc-attest. Run `anchor build` first (tests load target/deploy/rc_attest.so).

use {
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, system_program,
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    rc_attest::{CheckResult, Config, Rule, CONFIG_SEED, RESULT_SEED, RULE_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

struct Env {
    svm: LiteSVM,
    authority: Keypair,
    config: Pubkey,
}

fn rule_id(s: &str) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[..s.len()].copy_from_slice(s.as_bytes());
    id
}

fn rule_pda(id: [u8; 32], sql_hash: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[RULE_SEED, &id, &sql_hash], &rc_attest::id()).0
}

fn result_pda(rule: Pubkey, hash: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[RESULT_SEED, rule.as_ref(), &hash], &rc_attest::id()).0
}

fn send(svm: &mut LiteSVM, ix: Instruction, signer: &Keypair) -> Result<(), String> {
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[signer])
        .map_err(|e| e.to_string())?;
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")))
}

fn load<T: AccountDeserialize>(svm: &LiteSVM, key: &Pubkey) -> (T, usize) {
    let acc = svm.get_account(key).expect("account exists");
    let mut data: &[u8] = &acc.data;
    (
        T::try_deserialize(&mut data).expect("deserializes"),
        acc.data.len(),
    )
}

const BPF_LOADER_UPGRADEABLE: Pubkey =
    Pubkey::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");

fn program_data() -> Pubkey {
    Pubkey::find_program_address(&[rc_attest::id().as_ref()], &BPF_LOADER_UPGRADEABLE).0
}

/// LiteSVM deploys with no upgrade authority; patch the ProgramData metadata
/// (u32 tag, u64 slot, Option<Pubkey>) the way a real deploy sets it.
fn set_upgrade_authority(svm: &mut LiteSVM, authority: Pubkey) {
    let pd = program_data();
    let mut acc = svm.get_account(&pd).expect("program data");
    acc.data[12] = 1;
    acc.data[13..45].copy_from_slice(authority.as_ref());
    svm.set_account(pd, acc).expect("patch program data");
}

fn init_config_ix(config: Pubkey, authority: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        rc_attest::id(),
        &rc_attest::instruction::InitConfig {}.data(),
        rc_attest::accounts::InitConfig {
            config,
            authority,
            program: rc_attest::id(),
            program_data: program_data(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn register_ix(env: &Env, signer: Pubkey, id: [u8; 32], sql_hash: [u8; 32]) -> Instruction {
    Instruction::new_with_bytes(
        rc_attest::id(),
        &rc_attest::instruction::RegisterRule { id, sql_hash }.data(),
        rc_attest::accounts::RegisterRule {
            config: env.config,
            rule: rule_pda(id, sql_hash),
            authority: signer,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn post_ix(
    env: &Env,
    signer: Pubkey,
    rule: Pubkey,
    slot: u64,
    hash: [u8; 32],
    violated: bool,
) -> Instruction {
    Instruction::new_with_bytes(
        rc_attest::id(),
        &rc_attest::instruction::PostResult {
            slot,
            result_hash: hash,
            violated,
        }
        .data(),
        rc_attest::accounts::PostResult {
            config: env.config,
            rule,
            result: result_pda(rule, hash),
            authority: signer,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn setup() -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/rc_attest.so"
    ));
    svm.add_program(rc_attest::id(), bytes)
        .expect("program loads");
    let authority = Keypair::new();
    svm.airdrop(&authority.pubkey(), 10_000_000_000)
        .expect("airdrop");
    set_upgrade_authority(&mut svm, authority.pubkey());
    let config = Pubkey::find_program_address(&[CONFIG_SEED], &rc_attest::id()).0;
    let mut env = Env {
        svm,
        authority,
        config,
    };
    let ix = init_config_ix(env.config, env.authority.pubkey());
    send(&mut env.svm, ix, &env.authority).expect("init_config");
    env
}

fn funded(env: &mut Env) -> Keypair {
    let k = Keypair::new();
    env.svm
        .airdrop(&k.pubkey(), 10_000_000_000)
        .expect("airdrop");
    k
}

const SQL: [u8; 32] = [7u8; 32];
const HASH: [u8; 32] = [9u8; 32];

fn registered(env: &mut Env) -> Pubkey {
    let id = rule_id("R1_unbacked_mint");
    let ix = register_ix(env, env.authority.pubkey(), id, SQL);
    send(&mut env.svm, ix, &env.authority).expect("register");
    rule_pda(id, SQL)
}

#[test]
fn init_config_sets_authority_and_cannot_run_twice() {
    let mut env = setup();
    let (cfg, len) = load::<Config>(&env.svm, &env.config);
    assert_eq!(cfg.authority, env.authority.pubkey());
    assert_eq!(len, 8 + 32 + 1, "Config size");
    // Bug caught: config re-initialized by someone else to steal the authority.
    let attacker = funded(&mut env);
    let ix = init_config_ix(env.config, attacker.pubkey());
    assert!(send(&mut env.svm, ix, &attacker).is_err());
    assert_eq!(
        load::<Config>(&env.svm, &env.config).0.authority,
        env.authority.pubkey()
    );
}

#[test]
fn register_rule_stores_id_and_sql_hash() {
    let mut env = setup();
    let rule = registered(&mut env);
    let (r, len) = load::<Rule>(&env.svm, &rule);
    assert_eq!(r.id, rule_id("R1_unbacked_mint"));
    assert_eq!(r.sql_hash, SQL);
    assert_eq!(len, 8 + 32 + 32 + 1, "Rule size");
}

// Bug caught: a rule version silently overwritten (results would point at different SQL).
#[test]
fn same_rule_version_cannot_be_registered_twice_but_new_sql_gets_new_account() {
    let mut env = setup();
    registered(&mut env);
    let id = rule_id("R1_unbacked_mint");
    let again = register_ix(&env, env.authority.pubkey(), id, SQL);
    assert!(send(&mut env.svm, again, &env.authority).is_err());
    let new_sql = [8u8; 32];
    let v2 = register_ix(&env, env.authority.pubkey(), id, new_sql);
    send(&mut env.svm, v2, &env.authority).expect("new SQL version");
    assert_ne!(rule_pda(id, SQL), rule_pda(id, new_sql));
}

#[test]
fn non_authority_cannot_register_rules() {
    let mut env = setup();
    let attacker = funded(&mut env);
    let ix = register_ix(&env, attacker.pubkey(), rule_id("R9_evil"), SQL);
    let err = send(&mut env.svm, ix, &attacker).expect_err("unauthorized");
    assert!(
        err.contains("Unauthorized") || err.contains("6000"),
        "{err}"
    );
}

#[test]
fn empty_rule_id_is_rejected() {
    let mut env = setup();
    let ix = register_ix(&env, env.authority.pubkey(), [0u8; 32], SQL);
    let err = send(&mut env.svm, ix, &env.authority).expect_err("empty id");
    assert!(err.contains("EmptyRuleId") || err.contains("6001"), "{err}");
}

#[test]
fn post_result_records_hash_slot_and_verdict() {
    let mut env = setup();
    let rule = registered(&mut env);
    let ix = post_ix(&env, env.authority.pubkey(), rule, u64::MAX, HASH, true);
    send(&mut env.svm, ix, &env.authority).expect("post");
    let (r, len) = load::<CheckResult>(&env.svm, &result_pda(rule, HASH));
    assert_eq!(
        (r.rule, r.slot, r.result_hash, r.violated),
        (rule, u64::MAX, HASH, true)
    );
    assert_eq!(len, 8 + 32 + 8 + 32 + 1 + 8 + 1, "CheckResult size");
}

// Bug caught: the same evidence posted twice (inflating the alert history).
#[test]
fn same_result_hash_cannot_be_posted_twice() {
    let mut env = setup();
    let rule = registered(&mut env);
    let ix = post_ix(&env, env.authority.pubkey(), rule, 1, HASH, true);
    send(&mut env.svm, ix, &env.authority).expect("first");
    env.svm.expire_blockhash();
    let ix = post_ix(&env, env.authority.pubkey(), rule, 2, HASH, false);
    assert!(send(&mut env.svm, ix, &env.authority).is_err());
}

// Bug caught: anyone forging "all clear" results.
#[test]
fn non_authority_cannot_post_results() {
    let mut env = setup();
    let rule = registered(&mut env);
    let attacker = funded(&mut env);
    let ix = post_ix(&env, attacker.pubkey(), rule, 1, HASH, false);
    let err = send(&mut env.svm, ix, &attacker).expect_err("unauthorized");
    assert!(
        err.contains("Unauthorized") || err.contains("6000"),
        "{err}"
    );
}

// Bug caught: results attached to a rule that was never registered (no SQL hash on record).
#[test]
fn result_for_unregistered_rule_is_rejected() {
    let mut env = setup();
    let fake_rule = rule_pda(rule_id("R7_never_registered"), SQL);
    let ix = post_ix(&env, env.authority.pubkey(), fake_rule, 1, HASH, true);
    assert!(send(&mut env.svm, ix, &env.authority).is_err());
}

#[test]
fn all_zero_result_hash_is_rejected() {
    let mut env = setup();
    let rule = registered(&mut env);
    let ix = post_ix(&env, env.authority.pubkey(), rule, 1, [0u8; 32], true);
    let err = send(&mut env.svm, ix, &env.authority).expect_err("zero hash");
    assert!(
        err.contains("EmptyResultHash") || err.contains("6002"),
        "{err}"
    );
}

// Bug caught: anyone front-running init_config on a fresh deploy to become the attestation
// authority (review finding #8).
#[test]
fn only_upgrade_authority_can_init_config() {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/rc_attest.so"
    ));
    svm.add_program(rc_attest::id(), bytes)
        .expect("program loads");
    let deployer = Keypair::new();
    let attacker = Keypair::new();
    for k in [&deployer, &attacker] {
        svm.airdrop(&k.pubkey(), 10_000_000_000).expect("airdrop");
    }
    set_upgrade_authority(&mut svm, deployer.pubkey());
    let config = Pubkey::find_program_address(&[CONFIG_SEED], &rc_attest::id()).0;
    let err = send(
        &mut svm,
        init_config_ix(config, attacker.pubkey()),
        &attacker,
    )
    .expect_err("front-run");
    assert!(
        err.contains("NotUpgradeAuthority") || err.contains("6003"),
        "{err}"
    );
    send(
        &mut svm,
        init_config_ix(config, deployer.pubkey()),
        &deployer,
    )
    .expect("deployer inits");
}
