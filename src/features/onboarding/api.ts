import { invoke } from "@tauri-apps/api/core";

export interface OnboardingReadiness {
  localEnvironment: boolean;
  codingService: boolean;
  openaiTunnel: boolean;
}

export interface OnboardingState {
  complete: boolean;
  connectionConfigured: boolean;
  runtimeKeySaved: boolean;
  readiness: OnboardingReadiness;
}

export const onboardingApi = {
  read: () => invoke<OnboardingState>("get_onboarding_state"),
  saveConnection: (tunnelId: string, runtimeKey: string) => invoke<void>("save_onboarding_connection", { tunnelId, runtimeKey }),
  rememberProject: (path: string) => invoke<string>("add_project", { path, deferActivation: true }),
  startProject: (id: string) => invoke<void>("select_project", { id }),
  openChatGpt: () => invoke<void>("open_chatgpt_mcp_page"),
  complete: () => invoke<void>("complete_onboarding"),
};
