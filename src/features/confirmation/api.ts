import { invoke } from "@tauri-apps/api/core";

import { isRecord, isStringOrNull } from "../../bridge";

export interface PendingConfirmation {
  id: string;
  route: string;
  /** 命令原文。展示时不截断——摘要过的命令没法让人做判断。 */
  command: string;
  workdir: string | null;
  risk: string[];
  requestedAtMs: number;
  expiresAtMs: number;
}

export interface ConfirmationProjection {
  revision: number;
  entries: PendingConfirmation[];
}

const isEntry = (value: unknown): value is PendingConfirmation =>
  isRecord(value)
  && typeof value.id === "string"
  && typeof value.route === "string"
  && typeof value.command === "string"
  && isStringOrNull(value.workdir)
  && Array.isArray(value.risk) && value.risk.every((item) => typeof item === "string")
  && typeof value.requestedAtMs === "number"
  && typeof value.expiresAtMs === "number";

function parseConfirmations(value: unknown): ConfirmationProjection {
  if (!isRecord(value)
    || typeof value.revision !== "number"
    || !Array.isArray(value.entries)
    || !value.entries.every(isEntry)) {
    throw new Error("后端待确认记录合同不兼容");
  }
  return value as unknown as ConfirmationProjection;
}

export const confirmationApi = {
  read: async () => parseConfirmations(await invoke<unknown>("get_pending_confirmations")),
  waitForChange: (sinceRevision: number) => invoke<number>("wait_pending_confirmations_change", { sinceRevision }),
  approve: (id: string) => invoke<boolean>("approve_administrator_command", { id }),
  reject: (id: string) => invoke<boolean>("reject_administrator_command", { id }),
};
