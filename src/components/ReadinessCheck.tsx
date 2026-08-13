import type { ServiceCode } from "../bridge";
import { ServiceStatusDot } from "./ServiceStatusDot";

const readinessText: Record<ServiceCode, string> = {
  off: "等待检查",
  starting: "正在检查",
  online: "已通过",
  recovering: "正在检查",
  fault: "需要处理",
};

export function ReadinessCheck({ label, service }: { label: string; service: ServiceCode | null }) {
  return (
    <div className="readiness-row">
      <ServiceStatusDot service={service} />
      <span>{label}</span>
      <span className="readiness-state">{service === null ? "等待检查" : readinessText[service]}</span>
    </div>
  );
}
