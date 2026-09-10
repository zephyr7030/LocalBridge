import { useState } from "react";
import { bridge, type AccessCode, type ProjectProjection } from "../../bridge";
import { brandStatusText, overallServiceState, overallVisualState, uiErrorText } from "../../presentation";
import { uiText } from "../../presentation";
import { AdminModeWarning } from "../../components/AdminModeWarning";
import { ModalSurface } from "../../components/ModalSurface";
import { UiErrorNotice } from "../../components/UiErrorNotice";
import { Diagnostics } from "../diagnostics/Diagnostics";
import { ProjectPicker } from "./ProjectPicker";
import { SettingsSheet } from "./SettingsSheet";
import { ActivityFeed } from "./ActivityFeed";
import { ConfirmationRequests } from "./ConfirmationRequests";
import { ContextBar } from "./ContextBar";
import { ServiceDetails } from "./ServiceDetails";
import { StatusHeadline } from "./StatusHeadline";
import { useDashboardProjection } from "./useDashboardProjection";

type View = "main" | "settings" | "diagnostics";

export function Dashboard({ onOpenWelcome }: { onOpenWelcome: () => void }) {
  const { projection, transportError, error, run } = useDashboardProjection();
  const [view, setView] = useState<View>("main");
  const [removeTarget, setRemoveTarget] = useState<ProjectProjection | null>(null);
  const [projectPickerOpen, setProjectPickerOpen] = useState(false);
  const [adminWarningOpen, setAdminWarningOpen] = useState(false);
  const [fullAccessInfoOpen, setFullAccessInfoOpen] = useState(false);
  const [handledGeneration, setHandledGeneration] = useState<number | null>(null);
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);

  const activeProject = projection?.projects?.find((item) => item.active) ?? null;
  const adminModeFullAccess = projection?.pathAuthority === "administrator";
  const reconnectVisible = Boolean(
    projection?.reconnect && projection.reconnect.generation !== handledGeneration,
  );
  const availableUpdateVersion = projection?.update?.state === "available"
    ? projection.update.latestVersion
    : null;
  const updateDialogVisible = Boolean(
    view === "main"
      && availableUpdateVersion
      && availableUpdateVersion !== dismissedUpdateVersion
      && !projectPickerOpen
      && !removeTarget
      && !reconnectVisible
      && !adminWarningOpen
      && !fullAccessInfoOpen,
  );

  // Administrator is the one mode the user cannot select silently: it needs the
  // shared consent warning first, and the backend gate re-checks it anyway.
  const chooseAccess = (mode: AccessCode) => {
    if (mode === "admin" && projection?.privilege !== "active") {
      setAdminWarningOpen(true);
      return;
    }
    void run(() => bridge.setAccess(mode));
  };

  const openProjectPicker = () => {
    if (adminModeFullAccess) {
      setFullAccessInfoOpen(true);
      return;
    }
    setProjectPickerOpen(true);
  };

  return (
    <main className="shell">
      <header className="topbar">
        <div className="brand">
          <span
            className={`service-status-dot status-${overallVisualState[overallServiceState(projection)]}`}
            aria-hidden="true"
          />
          <span>{brandStatusText(projection)}</span>
        </div>
        <div className="top-actions">
          <button className="ghost" onClick={() => setView("settings")}>{uiText.settings}</button>
          <button className="ghost" onClick={() => setView("diagnostics")}>{uiText.diagnostics}</button>
        </div>
      </header>

            {/* 此刻在干什么，紧贴标题行。项目和权限是上下文，排在它后面。 */}
      <StatusHeadline projection={projection} />

      <ContextBar
        projection={projection}
        activeProject={activeProject}
        adminModeFullAccess={adminModeFullAccess}
        onOpenProjectPicker={openProjectPicker}
      />

      <ServiceDetails projection={projection} />

      <ActivityFeed />

      {projection?.activeFaults.length ? (
        <section className="fault-banner" role="alert">
          <div>
            <strong>LocalBridge 需要处理</strong>
            <p>
              {uiErrorText(projection.activeFaults[0])}
              {projection.activeFaults.length > 1 ? `（另有 ${projection.activeFaults.length - 1} 项）` : ""}
            </p>
          </div>
          <button className="secondary" onClick={() => setView("diagnostics")}>查看诊断</button>
        </section>
      ) : null}

      <div className="service-actions" aria-label="服务控制">
        <div className="service-actions-group">
          <button className="secondary service-restart" onClick={() => void run(() => bridge.restartServices())}>
            重启服务
          </button>
          <button className="secondary service-stop" onClick={() => void run(() => bridge.stopServices())}>
            关闭服务
          </button>
        </div>
      </div>

      {transportError && <UiErrorNotice error={transportError} />}
      {error && <UiErrorNotice error={error} />}

      {view === "settings" && (
        <SettingsSheet
          projection={projection}
          run={run}
          onClose={() => setView("main")}
          onOpenWelcome={onOpenWelcome}
          onChooseAccess={chooseAccess}
        />
      )}

      {view === "diagnostics" && <Diagnostics commandError={error} onClose={() => setView("main")} />}

      {updateDialogVisible && (
        <ModalSurface backdropClassName="update-dialog-backdrop" priority={15} labelledBy="update-dialog-title" onDismiss={() => setDismissedUpdateVersion(availableUpdateVersion)}>
            <h2 id="update-dialog-title">发现新版本</h2>
            <p>LocalBridge {availableUpdateVersion} 已发布。</p>
            <div className="dialog-actions">
              <button className="secondary" onClick={() => setDismissedUpdateVersion(availableUpdateVersion)}>
                稍后
              </button>
              <button
                className="primary"
                onClick={() =>
                  void run(async () => {
                    await bridge.openGitHubReleases();
                    setDismissedUpdateVersion(availableUpdateVersion);
                  })
                }
              >
                查看更新
              </button>
            </div>
        </ModalSurface>
      )}

      {/* 等人点头的管理员命令。窗口由后端观察者叫到前台，这里把它摆成
          一件必须回答的事。 */}
      <ConfirmationRequests />

      {adminWarningOpen && (
        <AdminModeWarning
          onCancel={() => setAdminWarningOpen(false)}
          onConfirm={() => {
            setAdminWarningOpen(false);
            void run(() => bridge.setAccess("admin"));
          }}
        />
      )}

      {fullAccessInfoOpen && (
        <ModalSurface labelledBy="full-access-title" onDismiss={() => setFullAccessInfoOpen(false)} dismissOnBackdrop>
            <h2 id="full-access-title">全目录访问</h2>
            <p>管理员模式拥有系统管理员令牌范围内的文件访问能力，若要切换，请切换其他模式</p>
            <div className="dialog-actions">
              <button className="primary" onClick={() => setFullAccessInfoOpen(false)}>完成</button>
            </div>
        </ModalSurface>
      )}

      {projectPickerOpen && (
        <ProjectPicker
          projection={projection}
          run={run}
          onClose={() => setProjectPickerOpen(false)}
          onConfirmRemoveActive={setRemoveTarget}
        />
      )}

      {removeTarget && (
        <ModalSurface labelledBy="remove-project-title" onDismiss={() => setRemoveTarget(null)}>
            <h2 id="remove-project-title">移除当前项目</h2>
            <p>
              从 LocalBridge 移除此项目？
              <br />
              不会删除项目文件。
            </p>
            <div className="dialog-actions">
              <button className="secondary" onClick={() => setRemoveTarget(null)}>取消</button>
              <button
                className="primary"
                onClick={() =>
                  void run(async () => {
                    await bridge.removeProject(removeTarget.id);
                    setRemoveTarget(null);
                  })
                }
              >
                移除
              </button>
            </div>
        </ModalSurface>
      )}

      {reconnectVisible && projection?.reconnect && (
        <ModalSurface labelledBy="reconnect-title">
            <h2 id="reconnect-title">连接失败</h2>
            <p>已自动重试 5 次。</p>
            <div className="dialog-actions">
              <button
                className="secondary"
                onClick={() =>
                  void run(async () => {
                    await bridge.retry();
                    setHandledGeneration(projection.reconnect?.generation ?? null);
                  })
                }
              >
                重试
              </button>
              <button
                className="primary"
                onClick={() => {
                  setHandledGeneration(projection.reconnect?.generation ?? null);
                  setView("diagnostics");
                }}
              >
                查看诊断
              </button>
            </div>
        </ModalSurface>
      )}

    </main>
  );
}
