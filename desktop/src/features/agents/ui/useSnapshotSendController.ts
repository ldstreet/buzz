/**
 * useSnapshotSendController
 *
 * Payload-agnostic upload → send controller for sharing a snapshot to a Buzz
 * channel or DM.  The caller supplies pre-encoded bytes + filename from the
 * Rust layer; this hook drives uploadMediaBytes → useSendMessageMutation with
 * honest progress and idempotent double-send protection.
 *
 * This hook does not know what kind of snapshot the bytes contain.  A future
 * team-snapshot or other payload can reuse it unchanged by passing different
 * bytes and a filename.  Hard-coded semantics for `.agent.*` live only in
 * the export-dialog layer above this hook.
 */

import * as React from "react";

import { uploadMediaBytes, type BlobDescriptor } from "@/shared/api/tauri";
import { buildOutgoingMessage } from "@/features/messages/lib/imetaMediaMarkdown";
import { useChannelsQuery } from "@/features/channels/hooks";
import { isModerationDm } from "@/features/moderation/lib/moderationDm";
import { useRelaySelfQuery } from "@/features/moderation/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useSendMessageMutation } from "@/features/messages/hooks";
import type { Channel } from "@/shared/api/types";

// ── Public types ──────────────────────────────────────────────────────────────

export type SendPhase =
  | "idle"
  | "preparing"
  | "uploading"
  | "sending"
  | "done"
  | "error";

export type SnapshotSendState = {
  phase: SendPhase;
  error: string | null;
};

/**
 * A joined, non-archived, non-moderation-DM destination: channelType "stream"
 * or "dm", isMember true, archivedAt null.
 *
 * Moderation DM exclusion requires the relay `self` pubkey and the current
 * user pubkey; those are applied in `useSendableChannels` below so callers
 * always receive a fully-filtered list.
 */
export function isSendableDestination(ch: Channel): boolean {
  return ch.isMember && ch.archivedAt === null && ch.channelType !== "forum";
}

export type UseSnapshotSendControllerResult = {
  /** Sendable destinations the user may select (loaded from the channel cache). */
  sendableChannels: Channel[];
  /** True while the channels query is loading for the first time. */
  isLoadingChannels: boolean;
  state: SnapshotSendState;
  /**
   * Directly set the send state. Used by the dialog layer to set the
   * `preparing` phase before invoking encode, so progress is honest.
   */
  setState: React.Dispatch<React.SetStateAction<SnapshotSendState>>;
  /**
   * Upload `bytes` and send them to `channelId` as a standard NIP-92 imeta
   * attachment message.
   *
   * The caller MUST have already obtained explicit destination-scoped
   * confirmation for memory-bearing payloads before calling this.  Returns
   * false and sets error state if a send is already in progress (double-send
   * guard) or if any step fails.  The method never throws.
   *
   * `channelId` is captured at call-time so a channel switch mid-send cannot
   * redirect the attachment (delegated to `useSendMessageMutation`).
   */
  sendPayload: (
    bytes: number[],
    filename: string,
    channelId: string,
  ) => Promise<boolean>;
  reset: () => void;
};

// ── Hook ──────────────────────────────────────────────────────────────────────

export function useSnapshotSendController(): UseSnapshotSendControllerResult {
  const channelsQuery = useChannelsQuery();
  const identityQuery = useIdentityQuery();
  // Only fetch relay self when there are DM candidates — same gate as ChannelPane.
  const hasDmCandidates = React.useMemo(
    () =>
      (channelsQuery.data ?? []).some(
        (ch) => ch.channelType === "dm" && isSendableDestination(ch),
      ),
    [channelsQuery.data],
  );
  const relaySelfQuery = useRelaySelfQuery(hasDmCandidates);

  const [state, setState] = React.useState<SnapshotSendState>({
    phase: "idle",
    error: null,
  });

  // Prevent double-send between renders.
  const inFlightRef = React.useRef(false);

  // Pass null channel here — we supply the captured channelId per-send instead.
  const sendMutation = useSendMessageMutation(null, identityQuery.data);

  const sendableChannels = React.useMemo(() => {
    const currentPubkey = identityQuery.data?.pubkey;
    const relaySelf = relaySelfQuery.data;
    return (channelsQuery.data ?? []).filter(
      (ch) =>
        isSendableDestination(ch) &&
        !isModerationDm(ch, currentPubkey, relaySelf),
    );
  }, [channelsQuery.data, identityQuery.data, relaySelfQuery.data]);

  async function sendPayload(
    bytes: number[],
    filename: string,
    channelId: string,
  ): Promise<boolean> {
    if (inFlightRef.current) return false;
    inFlightRef.current = true;

    try {
      // ── Upload ────────────────────────────────────────────────────────────
      setState({ phase: "uploading", error: null });

      let descriptor: BlobDescriptor;
      try {
        descriptor = await uploadMediaBytes(bytes, filename);
      } catch (err) {
        setState({
          phase: "error",
          error:
            err instanceof Error
              ? `Upload failed: ${err.message}`
              : "Upload failed.",
        });
        return false;
      }

      // Preserve the original filename in the descriptor so `buildImetaTags`
      // emits a `filename` field and the recipient's FileCard renders the
      // correct label.
      const descriptorWithFilename: BlobDescriptor = {
        ...descriptor,
        filename,
      };

      // ── Build message content + NIP-92 imeta tags ─────────────────────────
      const { content, mediaTags } = buildOutgoingMessage("", [
        descriptorWithFilename,
      ]);

      // ── Send to the captured destination via canonical mutation ────────────
      // Capturing channelId here prevents a channel switch from redirecting
      // the attachment; useSendMessageMutation resolves the live channel from
      // the query cache using the supplied id.
      setState({ phase: "sending", error: null });

      try {
        await sendMutation.mutateAsync({
          channelId,
          content,
          mediaTags: mediaTags ?? [],
        });
      } catch (err) {
        setState({
          phase: "error",
          error:
            err instanceof Error
              ? `Send failed: ${err.message}`
              : "Send failed.",
        });
        return false;
      }

      setState({ phase: "done", error: null });
      return true;
    } finally {
      inFlightRef.current = false;
    }
  }

  function reset() {
    if (!inFlightRef.current) {
      setState({ phase: "idle", error: null });
    }
  }

  return {
    sendableChannels,
    isLoadingChannels: channelsQuery.isLoading,
    state,
    setState,
    sendPayload,
    reset,
  };
}
