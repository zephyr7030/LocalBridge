import type { ServiceCode } from "../bridge";
import { ServiceStatusDot } from "./ServiceStatusDot";

const readinessText: Record<ServiceCode, string> = {
  off: "未启动",
  starting: "正在启动",
  online: "已就绪",
  recovering: "正在恢复",
  fault: "失败",
};

export function ReadinessCheck({ label, service }: { label: string; service: ServiceCode | null }) {
  return (
    <div className="readiness-row">
      <ServiceStatusDot service={service} />
      <span>{label}</span>
      <span className="readiness-state">{service === null ? "未知" : readinessText[service]}</span>
    </div>
  );
}
