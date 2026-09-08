import { useState } from "react";
import type { MainProjection, ProjectProjection, ServiceCode } from "../../bridge";
import {
  accessText,
  overallServiceDetail,
  overallServiceState,
  projectionStatusText,
  serviceText,
  workspaceDisplayText,
} from "../../presentation";
import { ServiceStatusDot } from "../../components/ServiceStatusDot";

const SERVICES: Array<[keyof MainProjection, string]> = [
  ["localEnvironmentService", "本地运行环境"],
  ["tunnelService", "OpenAI 安全隧道"],
  ["codingService", "编码服务"],
];

function ServiceDetailRow({
  label,
  service,
  projection,
}: {
  label: string;
  service: ServiceCode | null;
  projection: MainProjection | null;
}) {
  return (
    <div className="row service-detail-row">
      <span className="label">{label}</span>
      <span className="value service-value">
        <ServiceStatusDot service={service} />
        <span>
          {service
            ? serviceText[service]
            : projection
              ? projectionStatusText(projection.runtimeStatus)
              : "正在读取"}
        </span>
      </span>
    </div>
  );
}

/**
 * 项目、权限和运行状态是上下文，不是头条。三项服务折叠成一行；
 * 出故障时自动展开，因为那正是唯一需要看清是哪一项的时刻。
 */
export function StatusCard({
  projection,
  activeProject,
  adminModeFullAccess,
  onOpenProjectPicker,
}: {
  projection: MainProjection | null;
  activeProject: ProjectProjection | null;
  adminModeFullAccess: boolean;
  onOpenProjectPicker: () => void;
}) {
  const [manuallyExpanded, setManuallyExpanded] = useState(false);
  const faulted = overallServiceState(projection) === "fault";
  const expanded = manuallyExpanded || faulted;

  return (
    <section className="card">
      <div className="row">
        <span className="label">当前项目</span>
        <div className="project-actions">
          <span className={`value ${adminModeFullAccess ? "full-access" : ""}`}>
            {workspaceDisplayText(projection)}
          </span>
          <button
            className="secondary"
            disabled={projection?.settingsStatus !== "ready"}
            onClick={onOpenProjectPicker}
          >
            {activeProject ? "切换" : "选择项目"}
          </button>
        </div>
      </div>

      <div className="row">
        <span className="label">权限模式</span>
        <span className="value permission-mode-value">
          {projection?.effectivePermission
            ? accessText[projection.effectivePermission]
            : projection
              ? projectionStatusText(projection.authorityStatus)
              : "正在读取"}
        </span>
      </div>

      <div className="row">
        <span className="label">运行状态</span>
        <span className="value service-value">
          <span>{overallServiceDetail(projection) ?? "正在读取"}</span>
          <button
            className="ghost services-toggle"
            aria-expanded={expanded}
            aria-controls="service-details"
            disabled={faulted}
            onClick={() => setManuallyExpanded((current) => !current)}
          >
            {expanded ? "收起" : "详情"}
          </button>
        </span>
      </div>

      {expanded ? (
        <div id="service-details" className="service-details">
          {SERVICES.map(([key, label]) => (
            <ServiceDetailRow
              key={label}
              label={label}
              service={(projection?.[key] as ServiceCode | null | undefined) ?? null}
              projection={projection}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}
