#![cfg(test)]

use super::{Error, InvoiceRegistry, InvoiceRegistryClient};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String};

fn setup() -> (Env, InvoiceRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let client = InvoiceRegistryClient::new(&env, &env.register(InvoiceRegistry, ()));
    let committer = Address::generate(&env);
    (env, client, committer)
}

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn uri(env: &Env) -> String {
    String::from_str(env, "ipfs://bafy-invoice-metadata")
}

#[test]
fn commit_and_read_back() {
    let (env, client, committer) = setup();
    let h = hash(&env, 1);

    assert!(!client.has_invoice(&h));
    client.commit_invoice(&committer, &h, &uri(&env));
    assert!(client.has_invoice(&h));

    let record = client.get_invoice(&h);
    assert_eq!(record.committer, committer);
    assert_eq!(record.metadata_uri, uri(&env));
}

#[test]
fn commit_is_immutable() {
    let (env, client, committer) = setup();
    let h = hash(&env, 2);
    client.commit_invoice(&committer, &h, &uri(&env));

    // Re-committing the same hash fails, even by a different committer / new uri.
    let other = Address::generate(&env);
    let new_uri = String::from_str(&env, "ipfs://tampered");
    assert_eq!(
        client.try_commit_invoice(&other, &h, &new_uri),
        Err(Ok(Error::AlreadyCommitted))
    );
    // Original record is unchanged.
    assert_eq!(client.get_invoice(&h).committer, committer);
    assert_eq!(client.get_invoice(&h).metadata_uri, uri(&env));
}

#[test]
fn distinct_hashes_coexist() {
    let (env, client, committer) = setup();
    client.commit_invoice(&committer, &hash(&env, 3), &uri(&env));
    client.commit_invoice(&committer, &hash(&env, 4), &uri(&env));
    assert!(client.has_invoice(&hash(&env, 3)));
    assert!(client.has_invoice(&hash(&env, 4)));
}

#[test]
fn missing_invoice_errors() {
    let (env, client, _) = setup();
    assert!(matches!(
        client.try_get_invoice(&hash(&env, 9)),
        Err(Ok(Error::InvoiceNotFound))
    ));
    assert!(!client.has_invoice(&hash(&env, 9)));
}

#[test]
fn version_is_one() {
    let (_env, client, _) = setup();
    assert_eq!(client.version(), 1);
}
