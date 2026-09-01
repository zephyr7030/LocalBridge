import { invoke } from "@tauri-apps/api/core";
import { isNumberOrNull, isRecord, isStringOrNull } from "../../bridge";
export interface OnboardingReadiness {
  localEnvironment: boolean;
  codingService: boolean;
  openaiTunnel: boolean;
}

export interface OnboardingState {
  complete: boolean;
  projectionRevision: number;
  connectionConfigured: boolean;
  runtimeKeySaved: boolean;
  runtimeKeyLength: number | null;
  tunnelId: string | null;
  readiness: OnboardingReadiness;
}

export interface ConnectorEndpointProjection {
  endpoint: string | null;
}

export function parseOnboardingState(value: unknown): OnboardingState {
  if (!isRecord(value) || typeof value.complete !== "boolean"
    || typeof value.projectionRevision !== "number"
    || typeof value.connectionConfigured !== "boolean"
    || typeof value.runtimeKeySaved !== "boolean"
    || !isNumberOrNull(value.runtimeKeyLength)
    || !isStringOrNull(value.tunnelId)
    || !isRecord(value.readiness)
    || typeof value.readiness.localEnvironment !== "boolean"
    || typeof value.readiness.codingService !== "boolean"
    || typeof value.readiness.openaiTunnel !== "boolean") {
    throw new Error("后端首次设置状态合同不兼容");
  }
  return value as unknown as OnboardingState;
}

export function parseConnectorEndpoint(value: unknown): ConnectorEndpointProjection {
  if (!isRecord(value) || !isStringOrNull(value.endpoint)) throw new Error("后端连接地址合同不兼容");
  return value as unknown as ConnectorEndpointProjection;
}

export const onboardingApi = {
  read: async () => parseOnboardingState(await invoke<unknown>("get_onboarding_state")),
  saveConnection: (tunnelId: string, runtimeKey: string) => invoke<void>("save_onboarding_connection", { tunnelId, runtimeKey }),
  prepareProject: async (projectId: string | null, selectedFolder: string | null) => parseOnboardingState(await invoke<unknown>("prepare_onboarding_project", { projectId, selectedFolder })),
  chooseWorkspaceFolder: () => invoke<string | null>("choose_onboarding_workspace_folder"),
  openTunnelSettings: () => invoke<void>("open_openai_tunnel_settings"),
  openApiKeys: () => invoke<void>("open_openai_api_keys"),
  openPluginsSettings: () => invoke<void>("open_chatgpt_plugins_settings"),
  openConnectorSettings: () => invoke<void>("open_chatgpt_custom_connector_settings"),
  readConnectorEndpoint: async () => parseConnectorEndpoint(await invoke<unknown>("get_connector_endpoint")),
  complete: () => invoke<void>("complete_onboarding"),
};
