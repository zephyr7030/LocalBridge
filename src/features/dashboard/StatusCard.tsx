import type { MainProjection, ProjectProjection, ServiceCode } from "../../bridge";
import { accessText, projectionStatusText, serviceText, workspaceDisplayText } from "../../presentation";
import { ServiceStatusDot } from "../../components/ServiceStatusDot";

function ServiceRow({
  label,
  service,
  projection,
}: {
  label: string;
  service: ServiceCode | null;
  projection: MainProjection | null;
}) {
  return (
    <div className="row">
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
      <ServiceRow
        label="本地运行环境"
        service={projection?.localEnvironmentService ?? null}
        projection={projection}
      />
      <ServiceRow
        label="OpenAI 安全隧道"
        service={projection?.tunnelService ?? null}
        projection={projection}
      />
      <ServiceRow
        label="编码服务"
        service={projection?.codingService ?? null}
        projection={projection}
      />
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
    </section>
  );
}
