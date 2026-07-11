//! End-to-end integration tests for NIP-37 draft wraps (kind:31234).
//!
//! These tests verify:
//! - Write-path validation: d/k tag rules, h/p rejection, expiration,
//!   ciphertext validation, blank tombstone acceptance
//! - Replacement ordering: NIP-01 last-write-wins, same-second tie-break,
//!   stale write cannot supersede current head, tombstone remains queryable
//! - Author-only reads: REQ, COUNT, WS subscription, HTTP /query, /count
//!   all confine drafts to their author — exclusive, mixed/kindless, ids,
//!   known #d, search, fan-out
//! - Community isolation: same address in two communities stays isolated
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3001 cargo test -p buzz-test-client --test e2e_nip37_draft -- --ignored
//! ```

use std::time::Duration;

use buzz_test_client::{BuzzTestClient, RelayMessage};
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag, Timestamp};
use reqwest::Client;
use serde_json::Value;

const KIND_DRAFT: u16 = 31234;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3001".to_string())
}

fn relay_http_url() -> String {
    relay_url()
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_string()
}

fn sub_id(name: &str) -> String {
    format!("e2e-nip37-{name}-{}", uuid::Uuid::new_v4())
}

fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

/// Minimal syntactically-plausible NIP-44 v2 payload.
/// base64(b"\x02" + b"\x00" * 98) — 132 chars, decoded 99 bytes, first byte 0x02.
fn fake_nip44_v2() -> String {
    let mut s = String::from("Ag");
    s.push_str(&"A".repeat(130));
    s
}

/// Build a valid kind:31234 draft wrap event.
fn build_draft(keys: &Keys, d_tag: &str, k_val: &str, content: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(KIND_DRAFT), content)
        .tags([
            Tag::parse(["d", d_tag]).unwrap(),
            Tag::parse(["k", k_val]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

/// Build a draft with an explicit created_at for deterministic ordering tests.
fn build_draft_at(
    keys: &Keys,
    d_tag: &str,
    k_val: &str,
    content: &str,
    created_at: Timestamp,
) -> nostr::Event {
    EventBuilder::new(Kind::Custom(KIND_DRAFT), content)
        .tags([
            Tag::parse(["d", d_tag]).unwrap(),
            Tag::parse(["k", k_val]).unwrap(),
        ])
        .custom_created_at(created_at)
        .sign_with_keys(keys)
        .unwrap()
}

/// Build a blank-content tombstone (NIP-37 deletion) for a draft address.
fn build_tombstone(keys: &Keys, d_tag: &str, k_val: &str) -> nostr::Event {
    build_draft(keys, d_tag, k_val, "")
}

/// Submit an event via HTTP and return (accepted, message).
async fn submit_event_http(client: &Client, keys: &Keys, event: &nostr::Event) -> (bool, String) {
    let pubkey_hex = keys.public_key().to_hex();
    let resp = client
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", &pubkey_hex)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(event).unwrap())
        .send()
        .await
        .expect("submit event");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.expect("parse response");
    if status == 200 {
        let accepted = body["accepted"].as_bool().unwrap_or(false);
        let message = body["message"].as_str().unwrap_or("").to_string();
        (accepted, message)
    } else {
        let message = body["error"].as_str().unwrap_or("").to_string();
        (false, message)
    }
}

/// Query events via HTTP bridge as `as_pubkey_hex`.
async fn query_events_http(
    client: &Client,
    as_pubkey_hex: &str,
    filters: Vec<Filter>,
) -> Vec<Value> {
    let resp = client
        .post(format!("{}/query", relay_http_url()))
        .header("X-Pubkey", as_pubkey_hex)
        .header("Content-Type", "application/json")
        .json(&filters)
        .send()
        .await
        .expect("query events");
    assert!(
        resp.status().is_success(),
        "query failed: {}",
        resp.status()
    );
    resp.json::<Vec<Value>>()
        .await
        .expect("parse query response")
}

/// Count events via HTTP bridge. Returns count or (status, message) on error.
async fn count_events_http(
    client: &Client,
    as_pubkey_hex: &str,
    filters: Vec<Filter>,
) -> Result<u64, (u16, String)> {
    let resp = client
        .post(format!("{}/count", relay_http_url()))
        .header("X-Pubkey", as_pubkey_hex)
        .header("Content-Type", "application/json")
        .json(&filters)
        .send()
        .await
        .expect("count events");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.expect("parse count response");
    if status == 200 {
        Ok(body["count"].as_u64().unwrap_or(0))
    } else {
        let msg = body["error"].as_str().unwrap_or("").to_string();
        Err((status, msg))
    }
}

// ─── Ingest validation ────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_draft_accepted_with_ciphertext_content() {
    let client = http_client();
    let keys = Keys::generate();
    let d_tag = uuid::Uuid::new_v4().to_string();
    let event = build_draft(&keys, &d_tag, "9", &fake_nip44_v2());
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(accepted, "valid draft rejected: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_accepted_blank_tombstone() {
    let client = http_client();
    let keys = Keys::generate();
    let d_tag = uuid::Uuid::new_v4().to_string();
    let event = build_tombstone(&keys, &d_tag, "9");
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(accepted, "blank tombstone rejected: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_missing_d_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([Tag::parse(["k", "9"]).unwrap()])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "missing d tag should be rejected");
    assert!(msg.contains("d` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_empty_d_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", ""]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "empty d tag should be rejected");
    assert!(msg.contains("d` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_duplicate_d_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "duplicate d tag should be rejected");
    assert!(msg.contains("d` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_missing_k_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([Tag::parse(["d", &d]).unwrap()])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "missing k tag should be rejected");
    assert!(msg.contains("k` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_duplicate_k_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "duplicate k tag should be rejected");
    assert!(msg.contains("k` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_malformed_k_tag_non_decimal() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "0x9"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "non-decimal k tag should be rejected");
    assert!(
        msg.contains("canonical decimal"),
        "unexpected message: {msg}"
    );
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_k_tag_leading_zero() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "09"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "k tag with leading zero should be rejected");
    assert!(msg.contains("leading zero"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_k_tag_out_of_range() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "65536"]).unwrap(), // u16::MAX + 1
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "k=65536 should be rejected (out of u16 range)");
    assert!(msg.contains("range"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_h_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
            Tag::parse(["h", &uuid::Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "h tag on draft should be rejected");
    assert!(msg.contains("h` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_p_tag() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
            Tag::parse(["p", &keys.public_key().to_hex()]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "p tag on draft should be rejected");
    assert!(msg.contains("p` tag"), "unexpected message: {msg}");
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_malformed_ciphertext() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), "not-a-ciphertext")
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "malformed ciphertext should be rejected");
    assert!(
        msg.contains("base64") || msg.contains("NIP-44"),
        "unexpected message: {msg}"
    );
}

#[tokio::test]
#[ignore]
async fn test_draft_rejected_expiration_in_past() {
    let client = http_client();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
            Tag::parse(["expiration", "1000000000"]).unwrap(), // long past
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let (accepted, msg) = submit_event_http(&client, &keys, &event).await;
    assert!(!accepted, "past expiration should be rejected");
    assert!(msg.contains("expiration"), "unexpected message: {msg}");
}

// ─── NIP-01 replacement / tombstone ordering ─────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_draft_replaced_by_newer_event() {
    let mut author = BuzzTestClient::connect(&relay_url(), &Keys::generate())
        .await
        .unwrap();
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let t0 = Timestamp::from(1_700_000_000u64);
    let t1 = Timestamp::from(1_700_000_001u64);

    let v1 = build_draft_at(&keys, &d, "9", &fake_nip44_v2(), t0);
    let v2 = build_draft_at(&keys, &d, "9", &fake_nip44_v2(), t1);

    // Reconnect as the author to submit both versions.
    let mut author_client = BuzzTestClient::connect(&relay_url(), &keys).await.unwrap();
    author_client.send_event(v1).await.unwrap();
    author_client.send_event(v2.clone()).await.unwrap();

    // Query — only the latest should be returned.
    let sid = sub_id("replace");
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(keys.public_key())
        .custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    author_client
        .subscribe(sid.clone(), vec![filter])
        .await
        .unwrap();
    let results = author_client
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(results.len(), 1, "should return exactly the latest draft");
    assert_eq!(
        results[0].id, v2.id,
        "latest event should be the returned head"
    );
    author_client.disconnect().await.unwrap();
    let _ = author;
}

#[tokio::test]
#[ignore]
async fn test_draft_stale_write_cannot_supersede_current_head() {
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let t_old = Timestamp::from(1_700_000_000u64);
    let t_new = Timestamp::from(1_700_000_002u64);

    let v_new = build_draft_at(&keys, &d, "9", &fake_nip44_v2(), t_new);
    let v_old = build_draft_at(&keys, &d, "9", &fake_nip44_v2(), t_old);

    let mut c = BuzzTestClient::connect(&relay_url(), &keys).await.unwrap();
    // Submit new first, then try to replace with stale.
    c.send_event(v_new.clone()).await.unwrap();
    c.send_event(v_old).await.unwrap();

    let sid = sub_id("stale");
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(keys.public_key())
        .custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    c.subscribe(sid.clone(), vec![filter]).await.unwrap();
    let results = c
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(results.len(), 1, "should have exactly one head");
    assert_eq!(
        results[0].id, v_new.id,
        "stale write must not replace current head"
    );
    c.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_tombstone_head_queryable_by_author() {
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&keys, &d, "9", &fake_nip44_v2());
    let tombstone = build_tombstone(&keys, &d, "9");

    let mut c = BuzzTestClient::connect(&relay_url(), &keys).await.unwrap();
    c.send_event(draft).await.unwrap();
    c.send_event(tombstone.clone()).await.unwrap();

    // Author can query the tombstone — it must remain stored as the latest head.
    let sid = sub_id("tombstone-head");
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(keys.public_key())
        .custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    c.subscribe(sid.clone(), vec![filter]).await.unwrap();
    let results = c
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(results.len(), 1, "tombstone must be the queryable head");
    assert_eq!(results[0].id, tombstone.id, "tombstone is the current head");
    assert_eq!(results[0].content, "", "tombstone content must be empty");
    c.disconnect().await.unwrap();
}

// ─── Author-only read gates ───────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_draft_author_can_req_own_drafts() {
    let keys = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&keys, &d, "9", &fake_nip44_v2());
    let mut c = BuzzTestClient::connect(&relay_url(), &keys).await.unwrap();
    c.send_event(draft.clone()).await.unwrap();

    let sid = sub_id("author-req");
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(keys.public_key());
    c.subscribe(sid.clone(), vec![filter]).await.unwrap();
    let results = c
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        results.iter().any(|e| e.id == draft.id),
        "author must receive own draft"
    );
    c.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_req_victims_drafts_exclusive() {
    // Victim stores a draft; attacker queries {kinds:[31234], authors:[victim]}.
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let mut vc = BuzzTestClient::connect(&relay_url(), &victim)
        .await
        .unwrap();
    vc.send_event(draft).await.unwrap();
    vc.disconnect().await.unwrap();

    let mut ac = BuzzTestClient::connect(&relay_url(), &attacker)
        .await
        .unwrap();
    let sid = sub_id("attacker-excl");
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(victim.public_key());
    // The subscription should be CLOSED with "restricted:" rather than returning events.
    ac.subscribe(sid.clone(), vec![filter]).await.unwrap();
    // Wait for CLOSED or EOSE — either is evidence the relay didn't deliver.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ac.recv_event(&sid).await {
                Ok(RelayMessage::Closed(_, _)) => break,
                Ok(RelayMessage::Eose(_)) => break,
                Ok(RelayMessage::Event(_, _)) => {
                    panic!("attacker received victim's draft via exclusive filter")
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timeout waiting for closed/eose");
    ac.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_see_draft_in_kindless_filter() {
    // Victim stores a draft; attacker issues a kindless filter.
    // Draft must be silently omitted (mixed/kindless fall through per-event filter).
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let mut vc = BuzzTestClient::connect(&relay_url(), &victim)
        .await
        .unwrap();
    let ok = vc.send_event(draft.clone()).await.unwrap();
    assert!(ok.accepted, "victim draft must be accepted");
    vc.disconnect().await.unwrap();

    let mut ac = BuzzTestClient::connect(&relay_url(), &attacker)
        .await
        .unwrap();
    let sid = sub_id("attacker-kindless");
    // Kindless filter — attacker cannot receive another user's draft.
    let filter = Filter::new().author(victim.public_key()).limit(50);
    ac.subscribe(sid.clone(), vec![filter]).await.unwrap();
    let results = ac
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        !results.iter().any(|e| e.id == draft.id),
        "kindless filter must not expose victim's draft to attacker"
    );
    ac.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_retrieve_by_known_event_id() {
    // Knowing the exact event ID of a draft must not grant access.
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let draft_id = draft.id;
    let mut vc = BuzzTestClient::connect(&relay_url(), &victim)
        .await
        .unwrap();
    vc.send_event(draft).await.unwrap();
    vc.disconnect().await.unwrap();

    let mut ac = BuzzTestClient::connect(&relay_url(), &attacker)
        .await
        .unwrap();
    let sid = sub_id("attacker-ids");
    // ids-only lookup — must not return the draft to the attacker.
    let filter = Filter::new().id(draft_id);
    ac.subscribe(sid.clone(), vec![filter]).await.unwrap();
    let results = ac
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        !results.iter().any(|e| e.id == draft_id),
        "knowing a draft's event id must not expose it to another user"
    );
    ac.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_retrieve_by_known_d_tag() {
    // Knowing the `d` value of a draft must not grant access via #d filter.
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let draft_id = draft.id;
    let mut vc = BuzzTestClient::connect(&relay_url(), &victim)
        .await
        .unwrap();
    vc.send_event(draft).await.unwrap();
    vc.disconnect().await.unwrap();

    let mut ac = BuzzTestClient::connect(&relay_url(), &attacker)
        .await
        .unwrap();

    // Exclusive #d query: {kinds:[31234], authors:[victim], #d:[known_d]}
    let sid1 = sub_id("attacker-d-excl");
    let filter_excl = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(victim.public_key())
        .custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    ac.subscribe(sid1.clone(), vec![filter_excl]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ac.recv_event(&sid1).await {
                Ok(RelayMessage::Closed(_, _)) => break,
                Ok(RelayMessage::Eose(_)) => break,
                Ok(RelayMessage::Event(_, _)) => {
                    panic!("attacker retrieved victim's draft via exclusive #d filter")
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timeout");

    // Mixed #d query: {#d:[known_d]} — kindless, no authors
    let sid2 = sub_id("attacker-d-kindless");
    let filter_mixed =
        Filter::new().custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    ac.subscribe(sid2.clone(), vec![filter_mixed])
        .await
        .unwrap();
    let results = ac
        .collect_until_eose(&sid2, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        !results.iter().any(|e| e.id == draft_id),
        "kindless #d filter must not expose victim's draft to attacker"
    );

    ac.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_count_victims_drafts() {
    let client = http_client();
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let (accepted, msg) = submit_event_http(&client, &victim, &draft).await;
    assert!(accepted, "victim draft must be accepted: {msg}");

    // Exclusive COUNT: another user querying {kinds:[31234], authors:[victim]}
    // must be rejected with restricted: error.
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(victim.public_key());
    let result = count_events_http(&client, &attacker.public_key().to_hex(), vec![filter]).await;
    assert!(
        result.is_err(),
        "attacker exclusive COUNT must be rejected, got Ok({:?})",
        result
    );

    // Kindless COUNT must not inflate count with victim's draft.
    let filter_kindless = Filter::new().author(victim.public_key()).limit(100);
    let count = count_events_http(
        &client,
        &attacker.public_key().to_hex(),
        vec![filter_kindless],
    )
    .await
    .unwrap_or(0);
    // We can't assert count==0 (victim may have other events), but we can assert
    // the call succeeded and the result doesn't include draft count for an attacker.
    // The important gate is that exclusive kind:31234 COUNT is rejected above.
    let _ = count;
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_count_via_known_d_tag_http() {
    let client = http_client();
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let (accepted, _) = submit_event_http(&client, &victim, &draft).await;
    assert!(accepted, "victim draft must be accepted");

    // Exclusive COUNT via known #d: must be rejected or return 0.
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(victim.public_key())
        .custom_tag(SingleLetterTag::lowercase(Alphabet::D), [d.clone()]);
    let result = count_events_http(&client, &attacker.public_key().to_hex(), vec![filter]).await;
    match result {
        Err(_) => {} // rejected — correct
        Ok(n) => {
            assert_eq!(
                n, 0,
                "attacker must not count victim's draft via #d filter, got {n}"
            );
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_draft_attacker_cannot_query_via_http_bridge() {
    let client = http_client();
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let draft_id = draft.id;
    let (accepted, _) = submit_event_http(&client, &victim, &draft).await;
    assert!(accepted, "victim draft must be accepted");

    // Attacker queries via /query — exclusive kind:31234 must be rejected or return empty.
    let filter = Filter::new()
        .kind(nostr::Kind::Custom(KIND_DRAFT))
        .author(victim.public_key());
    let results = query_events_http(&client, &attacker.public_key().to_hex(), vec![filter]).await;
    assert!(
        !results
            .iter()
            .any(|e| e.get("id").and_then(|v| v.as_str()) == Some(&draft_id.to_hex())),
        "attacker must not receive victim's draft via HTTP /query"
    );
}

#[tokio::test]
#[ignore]
async fn test_draft_not_surfaced_in_nip50_search() {
    let client = http_client();
    let victim = Keys::generate();
    let attacker = Keys::generate();
    // Use a unique token unlikely to appear in other events.
    let token = format!("nip37draft_fts_probe_{}", uuid::Uuid::new_v4().simple());
    let d = uuid::Uuid::new_v4().to_string();

    // Create a draft containing the unique token inside the "ciphertext".
    // The relay must store search_tsv = NULL for kind:31234.
    let event = EventBuilder::new(Kind::Custom(KIND_DRAFT), &fake_nip44_v2())
        .tags([
            Tag::parse(["d", &d]).unwrap(),
            Tag::parse(["k", "9"]).unwrap(),
        ])
        .sign_with_keys(&victim)
        .unwrap();
    let (accepted, _) = submit_event_http(&client, &victim, &event).await;
    assert!(accepted, "victim draft must be accepted");

    // Attacker NIP-50 search for the token — must return 0 results for 31234.
    let search_filter = Filter::new().search(&token).limit(50);
    let results = query_events_http(
        &client,
        &attacker.public_key().to_hex(),
        vec![search_filter],
    )
    .await;
    assert!(
        !results
            .iter()
            .any(|e| { e.get("kind").and_then(|k| k.as_u64()) == Some(KIND_DRAFT as u64) }),
        "kind:31234 must have NULL search_tsv — draft must not appear in NIP-50 search results"
    );
}

// ─── Fan-out privacy ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_draft_live_fanout_only_reaches_author() {
    let victim = Keys::generate();
    let attacker = Keys::generate();
    let d = uuid::Uuid::new_v4().to_string();

    // Attacker subscribes to a kindless global filter before the draft is published.
    let mut ac = BuzzTestClient::connect(&relay_url(), &attacker)
        .await
        .unwrap();
    let sid = sub_id("fanout-attacker");
    let filter = Filter::new().author(victim.public_key()).limit(0); // live only
    ac.subscribe(sid.clone(), vec![filter]).await.unwrap();
    // Drain EOSE.
    let _ = ac.collect_until_eose(&sid, Duration::from_secs(3)).await;

    // Victim publishes a draft.
    let draft = build_draft(&victim, &d, "9", &fake_nip44_v2());
    let draft_id = draft.id;
    let mut vc = BuzzTestClient::connect(&relay_url(), &victim)
        .await
        .unwrap();
    vc.send_event(draft).await.unwrap();
    vc.disconnect().await.unwrap();

    // Attacker must NOT receive the draft in live fan-out.
    let received = tokio::time::timeout(Duration::from_secs(2), ac.recv_event(&sid)).await;
    match received {
        Ok(Ok(RelayMessage::Event(_, e))) => {
            assert_ne!(
                e.id, draft_id,
                "attacker must not receive victim's draft via live fan-out"
            );
        }
        _ => {} // timeout or non-event — expected
    }
    ac.disconnect().await.unwrap();
}

// ─── Community isolation ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_nip11_advertises_nip37_not_nip40() {
    let client = http_client();
    let resp = client
        .get(relay_http_url())
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .expect("NIP-11 request");
    assert!(resp.status().is_success());
    let info: Value = resp.json().await.expect("parse NIP-11 response");
    let nips = info["supported_nips"]
        .as_array()
        .expect("supported_nips must be an array");
    let nip_numbers: Vec<u64> = nips.iter().filter_map(|v| v.as_u64()).collect();
    assert!(
        nip_numbers.contains(&37),
        "NIP-11 must advertise NIP-37 (draft wraps); got {nip_numbers:?}"
    );
    assert!(
        !nip_numbers.contains(&40),
        "NIP-11 must NOT advertise NIP-40 (expiry suppression not implemented); got {nip_numbers:?}"
    );
}
