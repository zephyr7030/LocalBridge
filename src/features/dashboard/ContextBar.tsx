import type { MainProjection, ProjectProjection } from "../../bridge";
import { accessText, projectionStatusText, workspaceDisplayText } from "../../presentation";

/**
 * 项目和权限是上下文，不是内容。它们各占一整行是在跟活动记录抢版面，
 * 而用户真正要读的是下面那条流水。
 */
export function ContextBar({
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
  const permission = projection?.effectivePermission
    ? accessText[projection.effectivePermission]
    : projection
      ? projectionStatusText(projection.authorityStatus)
      : "正在读取";
  return (
    <div className="context-bar">
      <span className="context-left">
        <span className={`context-workspace ${adminModeFullAccess ? "full-access" : ""}`}>
          {workspaceDisplayText(projection)}
        </span>
        <span className="context-separator" aria-hidden="true">·</span>
        <span className="context-permission">{permission}</span>
      </span>
      <button
        className="ghost context-switch"
        disabled={projection?.settingsStatus !== "ready"}
        onClick={onOpenProjectPicker}
      >
        {activeProject ? "切换" : "选择项目"}
      </button>
    </div>
  );
}
