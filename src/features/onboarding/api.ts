import { invoke } from "@tauri-apps/api/core";
import type { AccessCode } from "../../bridge";

export interface OnboardingReadiness {
  localEnvironment: boolean;
  codingService: boolean;
  openaiTunnel: boolean;
}

export interface OnboardingState {
  complete: boolean;
  permission: AccessCode;
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

export const onboardingApi = {
  read: () => invoke<OnboardingState>("get_onboarding_state"),
  saveConnection: (tunnelId: string, runtimeKey: string) => invoke<void>("save_onboarding_connection", { tunnelId, runtimeKey }),
  prepareProject: (projectId: string | null, selectedFolder: string | null) => invoke<OnboardingState>("prepare_onboarding_project", { projectId, selectedFolder }),
  chooseWorkspaceFolder: () => invoke<string | null>("choose_onboarding_workspace_folder"),
  openTunnelSettings: () => invoke<void>("open_openai_tunnel_settings"),
  openApiKeys: () => invoke<void>("open_openai_api_keys"),
  openPluginsSettings: () => invoke<void>("open_chatgpt_plugins_settings"),
  openConnectorSettings: () => invoke<void>("open_chatgpt_custom_connector_settings"),
  readConnectorEndpoint: () => invoke<ConnectorEndpointProjection>("get_connector_endpoint"),
  complete: () => invoke<void>("complete_onboarding"),
};
