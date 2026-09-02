import { useEffect, useState } from "react";
import { parseUiError, type UiError } from "../../bridge";
import { diagnosticsApi, type DiagnosticsViewProjection } from "./api";
import { UiErrorNotice } from "../../components/UiErrorNotice";
import "./diagnostics.css";

const eventTime = (timestampMs: number) => new Date(timestampMs).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });

const brokerText = {
  off: "未启用",
  requested: "等待授权",
  awaiting: "等待系统授权",
  active: "已启用",
  fault: "故障",
  unavailable: "状态暂不可用",
} as const;

export function Diagnostics({ onClose, commandError }: { onClose: () => void; commandError: UiError | null }) {
  const [snapshot, setSnapshot] = useState<DiagnosticsViewProjection | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [transportError, setTransportError] = useState<UiError | null>(null);
  const [exportedPath, setExportedPath] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let projectionRevision = 0;
      let logRevision = 0;
      let retryDelayMs = 250;
      while (!cancelled) {
        try {
          const next = await diagnosticsApi.read();
          if (cancelled) return;
          setSnapshot(next);
          setTransportError(null);
          projectionRevision = next.projectionRevision;
          logRevision = next.logRevision;
          retryDelayMs = 250;
          await diagnosticsApi.waitForChange(projectionRevision, logRevision);
        } catch (value) {
          if (cancelled) return;
          setSnapshot(null);
          setTransportError(parseUiError(value, "无法读取诊断状态"));
          await new Promise((resolve) => window.setTimeout(resolve, retryDelayMs));
          retryDelayMs = Math.min(retryDelayMs * 2, 5000);
        }
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const exportReport = async () => {
    setError(null);
    try {
      setExportedPath(await diagnosticsApi.exportReport());
    } catch (value) {
      setError(parseUiError(value, "无法导出诊断信息"));
    }
  };

  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <section className="sheet diagnostics-sheet" onMouseDown={(event) => event.stopPropagation()} aria-label="诊断">
        <h2>诊断</h2>
        {!snapshot ? <p className="diagnostics-muted">正在检查…</p> : (
          <>
            {snapshot.activeFaults.length ? <div className="diagnostics-section"><h3>需要处理</h3><div className="diagnostics-checks">{snapshot.activeFaults.map((fault) => <div className="diagnostics-check" key={fault.code}><span className="diagnostics-dot error" aria-hidden="true"/><div><strong>{fault.message}</strong><small>{fault.code}{fault.retryable ? " · 可重试" : ""}</small></div></div>)}</div></div> : null}
            <div className="diagnostics-section">
              <h3>运行状态</h3>
              <div className="diagnostics-checks">{snapshot.checks.map((check) => (
                <div className="diagnostics-check" key={check.code}>
                  <span className={`diagnostics-dot ${check.level}`} aria-hidden="true" />
                  <div><strong>{check.label}</strong><small>{check.detail}</small></div>
                </div>
              ))}</div>
              <div className="diagnostics-check"><span className={`diagnostics-dot ${snapshot.privilege === "active" ? "ok" : snapshot.privilege === "fault" ? "error" : snapshot.privilege === "unavailable" ? "unknown" : "warning"}`} aria-hidden="true" /><div><strong>管理员权限</strong><small>{brokerText[snapshot.privilege]}</small></div></div>
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
        {commandError && <UiErrorNotice error={commandError} />}
        {transportError && <UiErrorNotice error={transportError} />}
        {error && <UiErrorNotice error={error} />}
        {exportedPath ? <p className="diagnostics-exported">已导出到：{exportedPath}</p> : null}
        <div className="dialog-actions diagnostics-actions">
          <button className="secondary" onClick={() => void diagnosticsApi.openLogs().catch((value) => setError(parseUiError(value, "无法打开日志")))}>生成并打开日志</button>
          <button className="secondary" onClick={() => void exportReport()}>导出诊断</button>
          <button className="primary" onClick={onClose}>完成</button>
        </div>
      </section>
    </div>
  );
}
