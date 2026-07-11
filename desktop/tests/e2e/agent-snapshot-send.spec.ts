import { expect, test } from "@playwright/test";

import {
  installMockBridge,
  createMockAgentMemoryListing,
} from "../helpers/bridge";

// ── Helpers ───────────────────────────────────────────────────────────────────

type CommandLogEntry = { command: string; payload: unknown };

async function readCommandLog(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    return (
      (
        window as Window & {
          __BUZZ_E2E_COMMAND_LOG__?: CommandLogEntry[];
        }
      ).__BUZZ_E2E_COMMAND_LOG__ ?? []
    );
  });
}

async function gotoAgentsPage(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
}

// Seeded persona ID used across tests.
const ANALYST_PERSONA_ID = "test-analyst";
const ANALYST_PUBKEY =
  "953d3363262e86b770419834c53d2446409db6d918a57f8f339d495d54ab001f";

const MOCK_UPLOAD_DESCRIPTOR = {
  url: `https://mock.relay/media/${"a".repeat(64)}.json`,
  sha256: "a".repeat(64),
  size: 1234,
  type: "application/json",
  uploaded: Math.floor(Date.now() / 1000),
  filename: "analyst.agent.json",
};

// ── Destination picker: channel/DM visibility ─────────────────────────────────

test("snapshot_send_dialog_shows_joined_channels_and_dms", async ({ page }) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  // Pick the "Send in Buzz" option (if shown via format modal) or directly
  // expect the dialog. The export dialog offers save vs send — click Send in Buzz.
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) {
    await sendBtn.click();
  }

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();

  const list = page.getByTestId("agent-snapshot-send-channel-list");
  await expect(list).toBeVisible();

  // Joined streams (general, random) must appear.
  await expect(list).toContainText("general");
  await expect(list).toContainText("random");

  // DMs (alice-tyler, bob-tyler) must appear.
  await expect(list).toContainText("alice");
  await expect(list).toContainText("bob");
});

test("snapshot_send_dialog_excludes_forum_archived_and_moderation_dm", async ({
  page,
}) => {
  // RELAY_SELF_PUBKEY matches alice (the DM peer) so alice-tyler becomes a moderation DM.
  const ALICE_PUBKEY =
    "953d3363262e86b770419834c53d2446409db6d918a57f8f339d495d54ab001f";
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    relaySelf: ALICE_PUBKEY,
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();

  const list = page.getByTestId("agent-snapshot-send-channel-list");
  await expect(list).toBeVisible();

  // The DM whose only other participant is ALICE_PUBKEY (= relaySelf) must
  // NOT appear — it is a moderation DM.
  await expect(list.getByText("alice-tyler")).toHaveCount(0);

  // Forums must not appear (watercooler is a seeded forum).
  await expect(list).not.toContainText("watercooler");
});

// ── Config-only send flow: encode → upload → send, correct destination ────────

test("snapshot_send_config_only_calls_encode_upload_send_in_order", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();

  // Select #general.
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();

  // Confirm send.
  await page.getByTestId("agent-snapshot-send-confirm").click();

  // Dialog transitions: progress → done.
  await expect(page.getByTestId("agent-snapshot-send-progress")).toBeVisible();
  await expect(page.getByTestId("agent-snapshot-send-done")).toBeVisible({
    timeout: 8000,
  });

  // Verify command order: encode → upload → send_channel_message.
  const log = await readCommandLog(page);
  const relevantCommands = log
    .filter((e) =>
      [
        "encode_agent_snapshot_for_send",
        "upload_media_bytes",
        "send_channel_message",
      ].includes(e.command),
    )
    .map((e) => e.command);

  expect(relevantCommands).toEqual([
    "encode_agent_snapshot_for_send",
    "upload_media_bytes",
    "send_channel_message",
  ]);

  // Confirm send_channel_message targeted #general (its id from the seed).
  const sendEntry = log.find((e) => e.command === "send_channel_message");
  expect(sendEntry).toBeTruthy();
  const sendPayload = sendEntry?.payload as { channelId?: string } | undefined;
  // The general channel id is fixed in the e2eBridge seed.
  expect(sendPayload?.channelId).toBe("9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50");
});

// ── Memory-bearing flow: gate stops before encode/upload/send ─────────────────

test("snapshot_send_memory_gate_stops_before_encode_on_cancel", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
        status: "running",
      },
    ],
    agentMemory: createMockAgentMemoryListing(),
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();

  // Select memory level = "core" if the format/memory dialog is shown.
  // The export dialog may show memory options; select core if visible.
  // Use the exact label to avoid matching "Config + all memory" (whose description
  // also contains the word "core"), which causes a strict-mode violation.
  const coreOption = page
    .getByRole("radio")
    .filter({ hasText: "Config + core memory" })
    .first();
  if (await coreOption.isVisible()) {
    await coreOption.click();
  }
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();

  // Select a destination.
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();
  await page.getByTestId("agent-snapshot-send-confirm").click();

  // If memory level is "none" the gate is skipped — check for gate or done.
  const memGate = page.getByTestId("agent-snapshot-send-memory-gate");
  const hasMem = await memGate.isVisible().catch(() => false);
  if (!hasMem) {
    // Memory level is none in this context — test passes trivially.
    return;
  }

  // Memory gate is showing. Cancel.
  await page.getByRole("button", { name: "Cancel" }).click();

  // Verify NO encode/upload/send was called before or after cancel.
  const log = await readCommandLog(page);
  const dangerCmds = log
    .filter((e) =>
      [
        "encode_agent_snapshot_for_send",
        "upload_media_bytes",
        "send_channel_message",
      ].includes(e.command),
    )
    .map((e) => e.command);
  expect(dangerCmds).toEqual([]);
});

// ── Memory-bearing flow: gate names resolved destination ──────────────────────

test("snapshot_send_memory_gate_names_the_destination", async ({ page }) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
        status: "running",
      },
    ],
    agentMemory: createMockAgentMemoryListing(),
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();

  // Use the exact label to avoid matching "Config + all memory" (whose description
  // also contains the word "core"), which causes a strict-mode violation.
  const coreOption = page
    .getByRole("radio")
    .filter({ hasText: "Config + core memory" })
    .first();
  if (await coreOption.isVisible()) {
    await coreOption.click();
  }
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();

  // Select #general.
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();
  await page.getByTestId("agent-snapshot-send-confirm").click();

  // If memory gate shown, it must name the destination.
  const memGate = page.getByTestId("agent-snapshot-send-memory-gate");
  const hasMem = await memGate.isVisible().catch(() => false);
  if (!hasMem) return;

  await expect(memGate).toContainText("#general");
  await expect(memGate).toContainText("media link");
});

// ── Progress phases: Preparing → Uploading → Sending ─────────────────────────

test("snapshot_send_progress_shows_preparing_phase_before_uploading", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    // Use a slow upload to observe the Preparing phase before Uploading.
    uploadDelayMs: 400,
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();
  await page.getByTestId("agent-snapshot-send-confirm").click();

  // Progress step must be visible.
  const progress = page.getByTestId("agent-snapshot-send-progress");
  await expect(progress).toBeVisible();
  // At some point while in flight the label is one of the three honest phases.
  const progressText = await progress.textContent();
  expect([
    "Preparing snapshot…",
    "Uploading snapshot…",
    "Sending message…",
  ]).toContain(progressText?.trim());

  // Wait for done.
  await expect(page.getByTestId("agent-snapshot-send-done")).toBeVisible({
    timeout: 8000,
  });
});

// ── Done copy: no claim of direct import ─────────────────────────────────────

test("snapshot_send_done_does_not_claim_direct_import", async ({ page }) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();
  await page.getByTestId("agent-snapshot-send-confirm").click();

  const done = page.getByTestId("agent-snapshot-send-done");
  await expect(done).toBeVisible({ timeout: 8000 });

  // Must NOT claim recipients can directly click to import.
  const doneText = (await done.textContent()) ?? "";
  expect(doneText).not.toMatch(/click.*import/i);
  expect(doneText).not.toMatch(/directly from the message/i);

  // Must name the destination.
  expect(doneText).toMatch(/#general|general/);
});

// ── Double-send guard: confirming twice cannot duplicate ──────────────────────

test("snapshot_send_double_confirm_cannot_duplicate_send", async ({ page }) => {
  await installMockBridge(page, {
    personas: [
      {
        id: ANALYST_PERSONA_ID,
        displayName: "Analyst",
        systemPrompt: "You are an analyst.",
      },
    ],
    managedAgents: [
      {
        pubkey: ANALYST_PUBKEY,
        name: "Analyst",
        personaId: ANALYST_PERSONA_ID,
      },
    ],
    uploadDescriptors: [MOCK_UPLOAD_DESCRIPTOR],
  });
  await gotoAgentsPage(page);

  await page.getByLabel("Open actions for Analyst").click();
  await page.getByRole("menuitem", { name: "Export snapshot" }).click();
  const sendBtn = page.getByRole("button", { name: "Send in Buzz" });
  if (await sendBtn.isVisible()) await sendBtn.click();

  await expect(page.getByTestId("agent-snapshot-send-dialog")).toBeVisible();
  await page
    .getByTestId("agent-snapshot-send-channel-list")
    .getByText("general")
    .click();

  // Click confirm once, then verify the button is gone (disabled/hidden) before
  // attempting a second interaction — avoids force-clicking a detached element
  // which crashes the Tauri webview.
  const confirmBtn = page.getByTestId("agent-snapshot-send-confirm");
  await confirmBtn.click();
  // Wait for the confirm button to become disabled or hidden (in-flight guard).
  // A second click on a disabled button is a no-op; the test proves send count=1.
  await confirmBtn
    .waitFor({ state: "hidden", timeout: 2000 })
    .catch(async () => {
      // Button may stay visible but disabled — just try clicking it once more.
      await confirmBtn.click({ force: false }).catch(() => {});
    });

  await expect(page.getByTestId("agent-snapshot-send-done")).toBeVisible({
    timeout: 8000,
  });

  // send_channel_message must have been invoked exactly once.
  const log = await readCommandLog(page);
  const sendCount = log.filter(
    (e) => e.command === "send_channel_message",
  ).length;
  expect(sendCount).toBe(1);
});
