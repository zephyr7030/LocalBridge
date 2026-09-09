import { useEffect, useState } from "react";
import { bridge, type AccessCode, type MainProjection } from "../../bridge";
import { accessText, permissionRestartNotice, projectionStatusText, uiText, updateStatusText } from "../../presentation";
import type { DashboardRun } from "./useDashboardProjection";

const ACCESS_MODES: AccessCode[] = ["edit", "full", "admin"];

/**
 * Every piece of state here is settings-sheet local: an unsaved field value or
 * a confirmation step. It lives with the sheet so closing the sheet discards it,
 * instead of the dashboard having to reset it from the outside.
 */
export function SettingsSheet({
  projection,
  run,
  onClose,
  onOpenWelcome,
  onChooseAccess,
}: {
  projection: MainProjection | null;
  run: DashboardRun;
  onClose: () => void;
  onOpenWelcome: () => void;
  onChooseAccess: (mode: AccessCode) => void;
}) {
  const [tunnelValue, setTunnelValue] = useState("");
  const [keyValue, setKeyValue] = useState("");
  const [editingTunnel, setEditingTunnel] = useState(false);
  const [editingKey, setEditingKey] = useState(false);
  const [confirmingKeyDelete, setConfirmingKeyDelete] = useState(false);
  const runtimeKeySaved = projection?.runtimeKeySaved;

  useEffect(() => {
    if (!runtimeKeySaved) setConfirmingKeyDelete(false);
  }, [runtimeKeySaved]);

  const permissionNotice = permissionRestartNotice(projection);
  const connectionReady = projection?.connectionStatus === "ready";

  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <section className="sheet" onMouseDown={(event) => event.stopPropagation()}>
        <h2>{uiText.settings}</h2>

        <section className="settings-section">
          <h3>常规</h3>
          <div className="row">
            <span>开机启动</span>
            <input
              type="checkbox"
              disabled={projection?.autoStart == null}
              checked={projection?.autoStart ?? false}
              onChange={(event) => void run(() => bridge.setAutoStart(event.target.checked))}
            />
          </div>
          <div className="row">
            <span>关闭窗口后继续运行</span>
            <input
              type="checkbox"
              disabled={projection?.closeWindowContinueRunning == null}
              checked={projection?.closeWindowContinueRunning ?? false}
              onChange={(event) =>
                void run(() => bridge.setCloseWindowContinueRunning(event.target.checked))
              }
            />
          </div>
        </section>

        <section className="settings-section">
          <h3>连接</h3>
          <div className="field">
            <label htmlFor="tunnel-id">Tunnel ID</label>
            {editingTunnel ? (
              <>
                <input
                  id="tunnel-id"
                  autoComplete="off"
                  value={tunnelValue}
                  onChange={(event) => setTunnelValue(event.target.value)}
                  placeholder="输入 Tunnel ID"
                />
                <div className="inline-actions">
                  <button
                    className="primary"
                    disabled={!tunnelValue.trim()}
                    onClick={() =>
                      void run(async () => {
                        await bridge.saveTunnelId(tunnelValue.trim());
                        setTunnelValue("");
                        setEditingTunnel(false);
                      })
                    }
                  >
                    保存
                  </button>
                  <button
                    className="secondary"
                    onClick={() => {
                      setTunnelValue("");
                      setEditingTunnel(false);
                    }}
                  >
                    取消
                  </button>
                </div>
              </>
            ) : (
              <div className="settings-summary">
                <span>
                  {!connectionReady
                    ? projection
                      ? projectionStatusText(projection.connectionStatus)
                      : "正在读取"
                    : (projection?.connection?.desiredTunnelId ?? "未保存")}
                </span>
                <button
                  className="secondary settings-replace"
                  disabled={!connectionReady}
                  onClick={() => {
                    setTunnelValue(projection?.connection?.desiredTunnelId ?? "");
                    setEditingTunnel(true);
                  }}
                >
                  更换
                </button>
              </div>
            )}
          </div>

          <div className="field">
            <label htmlFor="runtime-key">Runtime API Key</label>
            {editingKey ? (
              <>
                <input
                  id="runtime-key"
                  type="password"
                  autoComplete="off"
                  value={keyValue}
                  onChange={(event) => setKeyValue(event.target.value)}
                  placeholder="输入新的 Runtime API Key"
                />
                <div className="inline-actions">
                  <button
                    className="primary"
                    disabled={!keyValue.trim()}
                    onClick={() =>
                      void run(async () => {
                        await bridge.saveKey(keyValue);
                        setKeyValue("");
                        setEditingKey(false);
                      })
                    }
                  >
                    保存
                  </button>
                  <button
                    className="secondary"
                    onClick={() => {
                      setKeyValue("");
                      setEditingKey(false);
                    }}
                  >
                    取消
                  </button>
                </div>
              </>
            ) : confirmingKeyDelete && runtimeKeySaved ? (
              <div className="settings-summary settings-delete-confirm">
                <span>请确认从windows安全凭据中删除？</span>
                <button
                  className="secondary settings-delete-cancel"
                  onClick={() => setConfirmingKeyDelete(false)}
                >
                  取消
                </button>
                <button
                  className="secondary settings-confirm-delete"
                  onClick={() =>
                    void run(async () => {
                      await bridge.clearKey();
                      setConfirmingKeyDelete(false);
                    })
                  }
                >
                  确认
                </button>
              </div>
            ) : (
              <div className="settings-summary">
                <span>
                  {runtimeKeySaved == null
                    ? projection
                      ? projectionStatusText(projection.settingsStatus)
                      : "正在读取"
                    : runtimeKeySaved
                      ? "已保存"
                      : "未保存"}
                </span>
                {runtimeKeySaved && (
                  <button
                    className="secondary settings-clear"
                    onClick={() => setConfirmingKeyDelete(true)}
                  >
                    清除
                  </button>
                )}
                <button
                  className="secondary settings-replace"
                  disabled={runtimeKeySaved == null}
                  onClick={() => {
                    setConfirmingKeyDelete(false);
                    setKeyValue("");
                    setEditingKey(true);
                  }}
                >
                  更换
                </button>
              </div>
            )}
          </div>
        </section>

        <section className="settings-section">
          <h3>权限</h3>
          <div className="access-grid">
            {ACCESS_MODES.map((mode) => {
              const selected = projection?.effectivePermission === mode;
              const pending =
                projection?.permission === mode && projection?.permissionReconciliation !== "converged";
              return (
                <button
                  key={mode}
                  disabled={projection?.authorityStatus !== "ready"}
                  aria-pressed={selected}
                  className={`choice ${mode === "admin" ? "admin-choice" : ""} ${selected ? "selected" : ""} ${pending ? "pending" : ""}`}
                  onClick={() => onChooseAccess(mode)}
                >
                  {accessText[mode]}
                </button>
              );
            })}
          </div>
          {permissionNotice ? <p className="settings-status">{permissionNotice}</p> : null}
        </section>

        <section className="settings-section">
          <h3>关于</h3>
          <div className="settings-summary">
            <span>{updateStatusText(projection?.update ?? null, projection?.updateStatus ?? "unavailable")}</span>
            <div className="inline-actions">
              <button
                className="secondary"
                disabled={!projection?.update || projection.update.state === "checking" || projection.update.state === "source_unavailable"}
                onClick={() => void run(async () => { await bridge.retryUpdateCheck(); })}
              >
                检查更新
              </button>
              <button
                className="secondary"
                disabled={!projection?.update?.releaseUrl}
                onClick={() => void run(async () => { await bridge.openGitHubReleases(); })}
              >
                GitHub Releases
              </button>
            </div>
          </div>
        </section>

        <div className="dialog-actions">
          <button className="secondary" onClick={onOpenWelcome}>
            打开欢迎页
          </button>
          <button className="primary" onClick={onClose}>
            完成
          </button>
        </div>
      </section>
    </div>
  );
}
