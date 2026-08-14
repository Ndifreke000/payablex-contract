#![cfg(test)]

use super::{Error, MultisigTreasury, MultisigTreasuryClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, vec, Address, BytesN, Env, String, Vec,
};

struct Ctx<'a> {
    env: Env,
    client: MultisigTreasuryClient<'a>,
    contract_id: Address,
    admin: Address,
    signers: Vec<Address>,
    token: token::Client<'a>,
    token_id: Address,
}

const FUNDING: i128 = 1_000_000_000; // 100 USDC (7 decimals)

fn create_token<'a>(env: &Env, admin: &Address) -> (Address, token::Client<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let addr = sac.address();
    (addr.clone(), token::Client::new(env, &addr))
}

/// Set up a treasury with `n` signers, `threshold`, funded with `FUNDING` USDC.
fn setup(n: u32, threshold: u32) -> Ctx<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (token_id, token) = create_token(&env, &admin);
    // Mint control uses the SAC admin client.
    let sac_admin = token::StellarAssetClient::new(&env, &token_id);

    let contract_id = env.register(MultisigTreasury, ());
    let client = MultisigTreasuryClient::new(&env, &contract_id);

    let mut signers = Vec::new(&env);
    for _ in 0..n {
        signers.push_back(Address::generate(&env));
    }
    client.init(&admin, &token_id, &signers, &threshold);
    sac_admin.mint(&contract_id, &FUNDING);

    Ctx {
        env,
        client,
        contract_id,
        admin,
        signers,
        token,
        token_id,
    }
}

fn memo(env: &Env) -> String {
    String::from_str(env, "INV-1042")
}

fn invoice_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

fn signer(ctx: &Ctx, i: u32) -> Address {
    ctx.signers.get(i).unwrap()
}

// ---- Happy path & getters ----

#[test]
fn init_sets_config() {
    let ctx = setup(3, 2);
    assert_eq!(ctx.client.get_admin(), ctx.admin);
    assert_eq!(ctx.client.get_token(), ctx.token_id);
    assert_eq!(ctx.client.get_threshold(), 2);
    assert_eq!(ctx.client.get_signers().len(), 3);
    assert!(!ctx.client.is_paused());
    assert_eq!(ctx.client.version(), 1);
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING);
}

#[test]
fn full_2_of_3_payment_flow() {
    let ctx = setup(3, 2);
    let recipient = Address::generate(&ctx.env);
    let amount = 250_000_000; // 25 USDC

    // Proposer (signer 0) proposes and is auto-counted as one approval.
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &amount,
        &memo(&ctx.env),
    );
    assert_eq!(ctx.client.valid_approvals(&id), 1);

    // Second signer approves -> threshold met.
    ctx.client.approve(&signer(&ctx, 1), &id);
    assert_eq!(ctx.client.valid_approvals(&id), 2);

    ctx.client.execute_payment(&id);

    assert_eq!(ctx.token.balance(&recipient), amount);
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING - amount);
    assert!(ctx.client.get_proposal(&id).executed);
}

// ---- Init validation ----

#[test]
fn init_rejects_bad_configs() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let (token_id, _) = create_token(&env, &admin);
    let client = MultisigTreasuryClient::new(&env, &env.register(MultisigTreasury, ()));

    let s3: Vec<Address> = vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let empty: Vec<Address> = Vec::new(&env);

    // Empty signer set.
    assert_eq!(
        client.try_init(&admin, &token_id, &empty, &1),
        Err(Ok(Error::InvalidSigners))
    );
    // Threshold zero.
    assert_eq!(
        client.try_init(&admin, &token_id, &s3, &0),
        Err(Ok(Error::InvalidThreshold))
    );
    // Threshold above signer count.
    assert_eq!(
        client.try_init(&admin, &token_id, &s3, &4),
        Err(Ok(Error::InvalidThreshold))
    );
    // Too many signers (11 > MAX 10).
    let mut eleven = Vec::new(&env);
    for _ in 0..11 {
        eleven.push_back(Address::generate(&env));
    }
    assert_eq!(
        client.try_init(&admin, &token_id, &eleven, &2),
        Err(Ok(Error::InvalidSigners))
    );
    // Duplicate signer.
    let dup_addr = Address::generate(&env);
    let dups: Vec<Address> = vec![&env, dup_addr.clone(), dup_addr];
    assert_eq!(
        client.try_init(&admin, &token_id, &dups, &1),
        Err(Ok(Error::DuplicateSigner))
    );
}

#[test]
fn init_is_one_shot() {
    let ctx = setup(3, 2);
    assert_eq!(
        ctx.client
            .try_init(&ctx.admin, &ctx.token_id, &ctx.signers, &2),
        Err(Ok(Error::AlreadyInitialized))
    );
}

// ---- Adversarial: authorization ----

#[test]
fn non_signer_cannot_propose_or_approve() {
    let ctx = setup(3, 2);
    let outsider = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);

    assert_eq!(
        ctx.client.try_propose_payment(
            &outsider,
            &invoice_hash(&ctx.env),
            &recipient,
            &100,
            &memo(&ctx.env)
        ),
        Err(Ok(Error::NotAuthorized))
    );

    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    assert_eq!(
        ctx.client.try_approve(&outsider, &id),
        Err(Ok(Error::NotAuthorized))
    );
}

#[test]
fn admin_has_no_payment_power() {
    // Admin is not one of the signers -> cannot propose payments, has no drain path.
    let ctx = setup(3, 2);
    let recipient = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client.try_propose_payment(
            &ctx.admin,
            &invoice_hash(&ctx.env),
            &recipient,
            &FUNDING,
            &memo(&ctx.env)
        ),
        Err(Ok(Error::NotAuthorized))
    );
    // Treasury balance untouched.
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING);
}

// ---- Adversarial: threshold, replay, duplicates ----

#[test]
fn below_threshold_execute_fails() {
    let ctx = setup(3, 2);
    let recipient = Address::generate(&ctx.env);
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    // Only the proposer has approved (1 of 2).
    assert_eq!(
        ctx.client.try_execute_payment(&id),
        Err(Ok(Error::ThresholdNotMet))
    );
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING);
}

#[test]
fn double_execute_is_blocked() {
    let ctx = setup(2, 2);
    let recipient = Address::generate(&ctx.env);
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    ctx.client.approve(&signer(&ctx, 1), &id);
    ctx.client.execute_payment(&id);
    assert_eq!(
        ctx.client.try_execute_payment(&id),
        Err(Ok(Error::AlreadyExecuted))
    );
    // Recipient paid exactly once.
    assert_eq!(ctx.token.balance(&recipient), 100);
}

#[test]
fn signer_cannot_approve_twice() {
    let ctx = setup(3, 2);
    let recipient = Address::generate(&ctx.env);
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    // Proposer is already an approver.
    assert_eq!(
        ctx.client.try_approve(&signer(&ctx, 0), &id),
        Err(Ok(Error::DuplicateApproval))
    );
    // A fresh signer can approve once, then not again.
    ctx.client.approve(&signer(&ctx, 1), &id);
    assert_eq!(
        ctx.client.try_approve(&signer(&ctx, 1), &id),
        Err(Ok(Error::DuplicateApproval))
    );
}

#[test]
fn rejects_non_positive_amount() {
    let ctx = setup(2, 1);
    let recipient = Address::generate(&ctx.env);
    for bad in [0i128, -1, -100] {
        assert_eq!(
            ctx.client.try_propose_payment(
                &signer(&ctx, 0),
                &invoice_hash(&ctx.env),
                &recipient,
                &bad,
                &memo(&ctx.env)
            ),
            Err(Ok(Error::InvalidAmount))
        );
    }
}

#[test]
fn missing_proposal_reports_not_found() {
    let ctx = setup(2, 1);
    assert_eq!(
        ctx.client.try_execute_payment(&999),
        Err(Ok(Error::ProposalNotFound))
    );
    assert_eq!(
        ctx.client.try_approve(&signer(&ctx, 0), &999),
        Err(Ok(Error::ProposalNotFound))
    );
}

// ---- Adversarial: pause cannot move or freeze funds forever ----

#[test]
fn pause_blocks_execution_but_not_funds() {
    let ctx = setup(2, 2);
    let recipient = Address::generate(&ctx.env);
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    ctx.client.approve(&signer(&ctx, 1), &id);

    ctx.client.pause();
    assert!(ctx.client.is_paused());
    assert_eq!(ctx.client.try_execute_payment(&id), Err(Ok(Error::Paused)));
    // Funds are untouched while paused: nothing drained.
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING);
    assert_eq!(ctx.token.balance(&recipient), 0);

    // Unpause -> the same approved payment goes through.
    ctx.client.unpause();
    ctx.client.execute_payment(&id);
    assert_eq!(ctx.token.balance(&recipient), 100);
}

#[test]
fn pause_requires_admin_auth() {
    let ctx = setup(2, 2);
    ctx.client.pause();
    // The contract demanded the admin's authorization for the pause call.
    let auths = ctx.env.auths();
    assert_eq!(auths.first().unwrap().0, ctx.admin);
}

// ---- Signer-set governance ----

#[test]
fn signer_update_flow_changes_the_set() {
    let ctx = setup(3, 2);
    let new_signer = Address::generate(&ctx.env);
    let new_set: Vec<Address> = vec![
        &ctx.env,
        signer(&ctx, 0),
        signer(&ctx, 1),
        signer(&ctx, 2),
        new_signer.clone(),
    ];

    let id = ctx
        .client
        .propose_signer_update(&signer(&ctx, 0), &new_set, &3);
    ctx.client.approve(&signer(&ctx, 1), &id);
    ctx.client.execute_signer_update(&id);

    assert_eq!(ctx.client.get_signers().len(), 4);
    assert_eq!(ctx.client.get_threshold(), 3);
    // The newly added signer can now participate.
    let recipient = Address::generate(&ctx.env);
    let pid = ctx.client.propose_payment(
        &new_signer,
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    assert_eq!(ctx.client.valid_approvals(&pid), 1);
}

#[test]
fn signer_update_cannot_be_executed_as_payment() {
    let ctx = setup(2, 1);
    let new_set: Vec<Address> = vec![&ctx.env, signer(&ctx, 0)];
    let id = ctx
        .client
        .propose_signer_update(&signer(&ctx, 0), &new_set, &1);
    assert_eq!(
        ctx.client.try_execute_payment(&id),
        Err(Ok(Error::WrongProposalKind))
    );
}

#[test]
fn removed_signer_approval_no_longer_counts() {
    // 3 signers, threshold 2. A payment is approved by s0 and s1 (2/2, ready).
    // Then a governance update removes s1. The payment now has only 1 valid approval
    // and can no longer be executed -> stale approvals do not count.
    let ctx = setup(3, 2);
    let recipient = Address::generate(&ctx.env);
    let pay_id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    ctx.client.approve(&signer(&ctx, 1), &pay_id);
    assert_eq!(ctx.client.valid_approvals(&pay_id), 2);

    // Remove s1: new set is {s0, s2}, threshold 2.
    let new_set: Vec<Address> = vec![&ctx.env, signer(&ctx, 0), signer(&ctx, 2)];
    let upd_id = ctx
        .client
        .propose_signer_update(&signer(&ctx, 0), &new_set, &2);
    ctx.client.approve(&signer(&ctx, 2), &upd_id);
    ctx.client.execute_signer_update(&upd_id);

    // s1's approval on the payment is now stale.
    assert_eq!(ctx.client.valid_approvals(&pay_id), 1);
    assert_eq!(
        ctx.client.try_execute_payment(&pay_id),
        Err(Ok(Error::ThresholdNotMet))
    );
    assert_eq!(ctx.token.balance(&ctx.contract_id), FUNDING);
}

#[test]
fn signer_update_rejects_bad_configs() {
    let ctx = setup(3, 2);

    // Threshold zero.
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&signer(&ctx, 0), &ctx.signers, &0),
        Err(Ok(Error::InvalidThreshold))
    );
    // Threshold above the proposed signer count.
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&signer(&ctx, 0), &ctx.signers, &4),
        Err(Ok(Error::InvalidThreshold))
    );
    // Empty signer set.
    let empty: Vec<Address> = Vec::new(&ctx.env);
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&signer(&ctx, 0), &empty, &1),
        Err(Ok(Error::InvalidSigners))
    );
    // Duplicate signer in the proposed set.
    let dup: Vec<Address> = vec![&ctx.env, signer(&ctx, 0), signer(&ctx, 0)];
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&signer(&ctx, 0), &dup, &1),
        Err(Ok(Error::DuplicateSigner))
    );
    // Only a current signer may propose a rotation.
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&outsider, &ctx.signers, &2),
        Err(Ok(Error::NotAuthorized))
    );
}

#[test]
fn signer_update_cannot_remove_signers_below_the_threshold_it_keeps() {
    // 3 signers, threshold 2. Proposing to drop to a single signer while keeping
    // threshold 2 would make the treasury permanently un-executable -> rejected
    // up front rather than accepted and stuck.
    let ctx = setup(3, 2);
    let shrunk: Vec<Address> = vec![&ctx.env, signer(&ctx, 0)];
    assert_eq!(
        ctx.client
            .try_propose_signer_update(&signer(&ctx, 0), &shrunk, &2),
        Err(Ok(Error::InvalidThreshold))
    );
}

#[test]
fn signer_update_requires_full_current_threshold_to_execute() {
    // Rotation is the highest-risk operation: it must not execute on fewer
    // approvals than an ordinary payment would need.
    let ctx = setup(3, 2);
    let new_set: Vec<Address> = vec![&ctx.env, signer(&ctx, 0), signer(&ctx, 2)];
    let id = ctx
        .client
        .propose_signer_update(&signer(&ctx, 0), &new_set, &2);
    // Only the proposer has approved (1 of 2) -> execution must be rejected, and
    // the old signer set/threshold must remain in force.
    assert_eq!(
        ctx.client.try_execute_signer_update(&id),
        Err(Ok(Error::ThresholdNotMet))
    );
    assert_eq!(ctx.client.get_signers().len(), 3);
    assert_eq!(ctx.client.get_threshold(), 2);
}

#[test]
fn signer_update_double_execute_is_blocked() {
    let ctx = setup(2, 2);
    let new_set: Vec<Address> = vec![&ctx.env, signer(&ctx, 0), signer(&ctx, 1)];
    let id = ctx
        .client
        .propose_signer_update(&signer(&ctx, 0), &new_set, &2);
    ctx.client.approve(&signer(&ctx, 1), &id);
    ctx.client.execute_signer_update(&id);
    assert_eq!(
        ctx.client.try_execute_signer_update(&id),
        Err(Ok(Error::AlreadyExecuted))
    );
}

// ---- Events ----

#[test]
fn emits_events_across_lifecycle() {
    let ctx = setup(2, 2);
    let recipient = Address::generate(&ctx.env);
    let id = ctx.client.propose_payment(
        &signer(&ctx, 0),
        &invoice_hash(&ctx.env),
        &recipient,
        &100,
        &memo(&ctx.env),
    );
    ctx.client.approve(&signer(&ctx, 1), &id);
    ctx.client.execute_payment(&id);
    // The execute invocation publishes at least our `execute` event
    // (`env.events().all()` reports the most recent invocation's events).
    assert!(!ctx.env.events().all().is_empty());
}
