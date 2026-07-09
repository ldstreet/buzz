import { invokeTauri } from "./tauri";

export type MeshHealth =
  | { status: "ok"; reason?: null }
  | { status: "degraded" | "failed"; reason: string };

export type MeshModelOption = {
  id: string;
  name: string | null;
};

export type MeshServeTarget = {
  modelId: string;
  modelName: string | null;
  endpointAddr: string;
  nodeName: string | null;
  capacity: { vramGb: number | null } | null;
  reporterPubkey?: string | null;
  endpointId?: string | null;
  deviceId?: string | null;
  deviceName?: string | null;
};

export type MeshAvailability = {
  capable: boolean;
  admitted: boolean;
  available: boolean;
  reason: string | null;
  models: MeshModelOption[];
  serveTargets: MeshServeTarget[];
};

export type MeshNodeState =
  | "off"
  | "starting"
  | "running"
  | "stopping"
  | "failed";
export type MeshNodeMode = "serve" | "client";

export type StartMeshNodeRequest = {
  mode: MeshNodeMode;
  modelId?: string;
  maxVramGb?: number;
  joinToken?: string;
};

export type MeshNodeStatus = {
  state: MeshNodeState;
  mode: MeshNodeMode | null;
  health: MeshHealth;
  apiBaseUrl: string | null;
  consoleUrl: string | null;
  modelId: string | null;
  modelName: string | null;
  inviteToken?: string | null;
  endpointId?: string | null;
  deviceId?: string | null;
  deviceName?: string | null;
};

export type MeshCallMeNow = {
  v: 1;
  type: "buzz-iroh-call-me-now";
  peer_endpoint_addr: string;
  peer_endpoint_id?: string;
  attempt_id: string;
  expires_at: number;
};

export async function meshAvailability(): Promise<MeshAvailability> {
  return await invokeTauri<MeshAvailability>("mesh_availability");
}

export async function meshStartNode(
  request: StartMeshNodeRequest,
): Promise<MeshNodeStatus> {
  return await invokeTauri<MeshNodeStatus>("mesh_start_node", { request });
}

export async function meshEnsureClientNode(
  modelId: string,
  target?: MeshServeTarget | null,
): Promise<MeshNodeStatus> {
  return await invokeTauri<MeshNodeStatus>("mesh_ensure_client_node", {
    request: {
      modelId,
      endpointAddr: target?.endpointAddr,
      reporterPubkey: target?.reporterPubkey,
      peerEndpointId: target?.endpointId,
    },
  });
}

export async function meshDialEndpointAddr(
  endpointAddr: string,
): Promise<MeshNodeStatus> {
  return await invokeTauri<MeshNodeStatus>("mesh_dial_endpoint_addr", {
    request: { endpointAddr },
  });
}

export async function meshStatusReportPayload(): Promise<Record<
  string,
  unknown
> | null> {
  return await invokeTauri<Record<string, unknown> | null>(
    "mesh_status_report_payload",
  );
}

export async function meshStopNode(): Promise<MeshNodeStatus> {
  return await invokeTauri<MeshNodeStatus>("mesh_stop_node");
}

export async function meshNodeStatus(): Promise<MeshNodeStatus> {
  return await invokeTauri<MeshNodeStatus>("mesh_node_status");
}

export async function meshInstalledModels(): Promise<MeshModelOption[]> {
  return await invokeTauri<MeshModelOption[]>("mesh_installed_models");
}

export type MeshModelFit = "comfortable" | "tight" | "tradeoff" | "too_large";

export type MeshCatalogEntry = {
  /** Catalog name — valid as-is in the model field. */
  name: string;
  /** Display size, e.g. "5.0GB". */
  size: string;
  sizeGb: number;
  description: string;
  fit: MeshModelFit;
  installed: boolean;
  recommended: boolean;
};

export type MeshModelCatalog = {
  gpuName: string | null;
  vramDisplay: string;
  vramGb: number;
  recommended: string | null;
  /** Ranked: recommended first, then by fit, then larger first within a fit. */
  entries: MeshCatalogEntry[];
};

/**
 * Hardware-aware curated model catalog for the Share-compute picker.
 * Works without a running mesh node (hardware survey + HF cache scan).
 */
export async function meshModelCatalog(): Promise<MeshModelCatalog> {
  return await invokeTauri<MeshModelCatalog>("mesh_model_catalog");
}
