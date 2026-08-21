import type { ServiceCode } from "../bridge";
import { serviceVisualState } from "../presentation";

export function ServiceStatusDot({ service }: { service: ServiceCode | null }) {
  const state = service === null ? "unknown" : serviceVisualState[service];
  return <span className={`service-status-dot status-${state}`} data-service-state={state} aria-hidden="true" />;
}
