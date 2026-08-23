import { useEffect, useState } from "react";
import { diagnosticsApi, type DiagnosticsViewProjection } from "./api";
import "./diagnostics.css";

const eventTime = (timestampMs: number) => new Date(timestampMs).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });

const brokerText = {
  off: "未启用",
  requested: "等待授权",
  awaiting: "等待系统授权",
  active: "已启用",
  fault: "故障",
} as const;

export function Diagnostics({ onClose }: { onClose: () => void }) {
  const [snapshot, setSnapshot] = useState<DiagnosticsViewProjection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exportedPath, setExportedPath] = useState<string | null>(null);

  useEffect(() => {
    void diagnosticsApi.read().then(setSnapshot).catch((value) => {
      setError(typeof value === "string" ? value : "无法读取诊断状态");
    });
  }, []);

  const exportReport = async () => {
    setError(null);
    try {
      setExportedPath(await diagnosticsApi.exportReport());
    } catch (value) {
      setError(typeof value === "string" ? value : "无法导出诊断信息");
    }
  };

  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <section className="sheet diagnostics-sheet" onMouseDown={(event) => event.stopPropagation()} aria-label="诊断">
        <h2>诊断</h2>
        {!snapshot ? <p className="diagnostics-muted">正在检查…</p> : (
          <>
            <div className="diagnostics-section">
              <h3>运行状态</h3>
              <div className="diagnostics-checks">{snapshot.checks.map((check) => (
                <div className="diagnostics-check" key={check.code}>
                  <span className={`diagnostics-dot ${check.level}`} aria-hidden="true" />
                  <div><strong>{check.label}</strong><small>{check.detail}</small></div>
                </div>
              ))}</div>
              <div className="diagnostics-check"><span className={`diagnostics-dot ${snapshot.privilege === "active" ? "ok" : snapshot.privilege === "fault" ? "error" : "warning"}`} aria-hidden="true" /><div><strong>管理员权限</strong><small>{brokerText[snapshot.privilege]}</small></div></div>
            </div>
            <div className="diagnostics-section">
              <h3>项目</h3>
              <div className="diagnostics-line"><span>当前项目</span><span>{snapshot.activeWorkspacePath ?? "未选择项目"}</span></div>
            </div>
            <div className="diagnostics-section">
              <h3>日志</h3>
              <div className="diagnostics-events">{snapshot.recentEvents.map((event, index) => <div className="diagnostics-check diagnostics-event" key={`${event.timestampMs}-${index}-${event.message}`}><span className={`diagnostics-dot ${event.level}`} aria-hidden="true"/><div className="diagnostics-event-body"><time dateTime={new Date(event.timestampMs).toISOString()}>{eventTime(event.timestampMs)}</time><span>{event.message}</span></div></div>)}</div>
            </div>
          </>
        )}
        {error ? <p className="diagnostics-error" role="alert">{error}</p> : null}
        {exportedPath ? <p className="diagnostics-exported">已导出到：{exportedPath}</p> : null}
        <div className="dialog-actions diagnostics-actions">
          <button className="secondary" onClick={() => void diagnosticsApi.openLogs().catch((value) => setError(typeof value === "string" ? value : "无法打开日志"))}>打开日志</button>
          <button className="secondary" onClick={() => void exportReport()}>导出诊断</button>
          <button className="primary" onClick={onClose}>完成</button>
        </div>
      </section>
    </div>
  );
}
