//! Unit tests for `mesh_llm/mod.rs` private helpers (kept in a sibling file so
//! `mod.rs` stays under the 500-line budget; `#[path]`-included from there).
use super::{find_progressish_reason, looks_like_model_ref};
use serde_json::json;

#[test]
fn progressish_reads_typed_phase_not_whole_tree() {
    assert_eq!(
        find_progressish_reason(&json!({"phase": "downloading weights"})),
        Some("downloading model".to_string())
    );
    // Regression (Sami N1): an unrelated field mentioning a progress word must
    // not trip the badge — only the typed phase field counts.
    assert_eq!(
        find_progressish_reason(&json!({
            "phase": "ready",
            "model_name": "prepared-qwen-preparing"
        })),
        None
    );
    assert_eq!(find_progressish_reason(&json!({"foo": "bar"})), None);
}

#[test]
fn model_ref_is_family_agnostic() {
    assert!(looks_like_model_ref("hf://org/model"));
    assert!(looks_like_model_ref("some-model.gguf"));
    assert!(looks_like_model_ref("Some-Model.GGUF"));
    // Families that used to be hardcoded must route via the structured path,
    // not a name allowlist here (Sami N2):
    assert!(!looks_like_model_ref("Mistral-7B"));
    assert!(!looks_like_model_ref("Qwen3-35B"));
    assert!(!looks_like_model_ref(""));
}

#[test]
fn iroh_relay_mode_defaults_to_enabled() {
    // Default is ON: unset, empty, "1", and "default" all enable the SDK's
    // default iroh relays, so members connect regardless of NAT. Relays are
    // transport-only (ciphertext forwarding) — admission is a separate layer.
    use super::IrohRelayMode;
    assert_eq!(super::iroh_relay_mode_from(None), IrohRelayMode::Default);
    assert_eq!(
        super::iroh_relay_mode_from(Some("")),
        IrohRelayMode::Default
    );
    assert_eq!(
        super::iroh_relay_mode_from(Some("  ")),
        IrohRelayMode::Default
    );
    assert_eq!(
        super::iroh_relay_mode_from(Some("1")),
        IrohRelayMode::Default
    );
    assert_eq!(
        super::iroh_relay_mode_from(Some("default")),
        IrohRelayMode::Default
    );
}

#[test]
fn iroh_relay_mode_opt_out_and_custom() {
    use super::IrohRelayMode;
    // "0" is the explicit opt-out for metadata-conscious deployments.
    assert_eq!(
        super::iroh_relay_mode_from(Some("0")),
        IrohRelayMode::Disabled
    );
    // Anything else is a comma-separated custom relay list.
    assert_eq!(
        super::iroh_relay_mode_from(Some("https://relay1.example, https://relay2.example ,")),
        IrohRelayMode::Custom(vec![
            "https://relay1.example".to_string(),
            "https://relay2.example".to_string(),
        ])
    );
}

#[test]
fn normalized_roster_none_means_no_enforcement() {
    let identity = super::identity::OwnerIdentity {
        keystore_path: std::path::PathBuf::from("/tmp/ks.json"),
        owner_id: "owner-self".to_string(),
    };
    assert_eq!(super::normalized_roster(&None, &identity), None);
}

#[test]
fn normalized_roster_always_includes_self_and_dedupes() {
    let identity = super::identity::OwnerIdentity {
        keystore_path: std::path::PathBuf::from("/tmp/ks.json"),
        owner_id: "owner-self".to_string(),
    };
    // Empty roster (fresh relay, nobody published yet) still admits self —
    // otherwise the first sharer locks themselves out.
    assert_eq!(
        super::normalized_roster(&Some(vec![]), &identity),
        Some(vec!["owner-self".to_string()])
    );
    // Dedup + trim + sorted, self merged in.
    assert_eq!(
        super::normalized_roster(
            &Some(vec![
                "owner-b".to_string(),
                " owner-a ".to_string(),
                "owner-b".to_string(),
                "".to_string(),
                "owner-self".to_string(),
            ]),
            &identity
        ),
        Some(vec![
            "owner-a".to_string(),
            "owner-b".to_string(),
            "owner-self".to_string(),
        ])
    );
}

fn signed_status_event(content: serde_json::Value) -> nostr::Event {
    let keys = nostr::Keys::generate();
    nostr::EventBuilder::new(nostr::Kind::Custom(30_621), content.to_string())
        .sign_with_keys(&keys)
        .expect("test event signs")
}

#[test]
fn owner_ids_from_events_collects_sorted_deduped_roster() {
    let events = vec![
        signed_status_event(json!({ "ownerId": "owner-b", "serveTargets": [] })),
        signed_status_event(json!({ "owner_id": "owner-a" })), // snake_case accepted
        signed_status_event(json!({ "ownerId": "owner-b" })),  // duplicate
        signed_status_event(json!({ "serveTargets": [] })),    // pre-upgrade note: no owner id
        signed_status_event(json!({ "ownerId": "" })),         // empty filtered
    ];
    assert_eq!(
        super::owner_ids_from_events(&events),
        vec!["owner-a".to_string(), "owner-b".to_string()]
    );
    assert_eq!(super::owner_ids_from_events(&[]), Vec::<String>::new());
}
