import assert from "node:assert/strict";
import test from "node:test";

import {
  canSubmitWhereToRun,
  emptyWhereToRunDraft,
  providerConfigComplete,
  resolveBackendIntent,
} from "./whereToRunIntent.ts";

const probed = {
  ok: true,
  config_schema: {
    properties: { region: { type: "string" }, size: { type: "integer" } },
    required: ["region"],
  },
};

function providerDraft(overrides = {}) {
  return {
    ...emptyWhereToRunDraft,
    runOn: "blox",
    probedProvider: probed,
    providerConfig: { region: "us", size: "3" },
    ...overrides,
  };
}

// ── Stale-intent edge (Pinky pin 2) ─────────────────────────────────────────

test("start toggle off discards a provider selection at submit", () => {
  assert.equal(
    resolveBackendIntent(providerDraft(), false),
    null,
    "definition-only create must never carry a backend intent",
  );
});

test("start toggle off always allows submit regardless of draft state", () => {
  // Incomplete provider config with the toggle off: no instance is minted,
  // so the draft must not block the definition-only create.
  const incomplete = providerDraft({ providerConfig: {} });
  assert.equal(canSubmitWhereToRun(incomplete, false), true);
});

// ── Submit gating carries over (Pinky pin 3) ────────────────────────────────

test("provider selection blocks submit until the probe completes", () => {
  const unprobed = providerDraft({ probedProvider: null });
  assert.equal(canSubmitWhereToRun(unprobed, true), false);
});

test("provider selection blocks submit while required config is missing", () => {
  const missing = providerDraft({ providerConfig: { size: "3" } });
  assert.equal(canSubmitWhereToRun(missing, true), false);
  assert.equal(providerConfigComplete(missing), false);
});

test("complete provider config allows submit", () => {
  assert.equal(canSubmitWhereToRun(providerDraft(), true), true);
});

test("local never gates submit", () => {
  assert.equal(canSubmitWhereToRun(emptyWhereToRunDraft, true), true);
});

// ── Intent resolution ────────────────────────────────────────────────────────

test("local draft resolves to null intent", () => {
  assert.equal(resolveBackendIntent(emptyWhereToRunDraft, true), null);
});

test("provider draft resolves with coerced config values", () => {
  const intent = resolveBackendIntent(providerDraft(), true);
  assert.deepEqual(intent, {
    type: "provider",
    id: "blox",
    config: { region: "us", size: 3 },
  });
});
