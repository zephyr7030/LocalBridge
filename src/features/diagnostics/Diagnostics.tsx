import { useCallback, useEffect, useState } from "react";
import { diagnosticsApi, type DiagnosticsSnapshot } from "./api";
import "./diagnostics.css";

const brokerText = {
  off: "未启用",
  requested: "等待授权",
  awaiting: "等待系统授权",
  active: "已启用",
  fault: "故障",
} as const;

const attemptText = { running: "进行中", failed: "失败" } as const;

export function Diagnostics({ onClose, onOpenWelcome }: { onClose: () => void; onOpenWelcome: () => void }) {
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exportedPath, setExportedPath] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      setSnapshot(await diagnosticsApi.read());
    } catch (value) {
      setError(typeof value === "string" ? value : "无法读取诊断状态");
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const retry = async () => {
    setError(null);
    try {
      await diagnosticsApi.retry();
      await refresh();
    } catch (value) {
      setError(typeof value === "string" ? value : "当前连接无法重试");
    }
  };

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
            <div className="diagnostics-checks">
              {snapshot.checks.map((check) => (
                <div className="diagnostics-check" key={check.code}>
                  <span className={`diagnostics-dot ${check.level}`} aria-hidden="true" />
                  <div><strong>{check.label}</strong><small>{check.detail}</small></div>
                </div>
              ))}
            </div>
            <div className="diagnostics-section">
              <h3>管理员组件</h3>
              <div className="diagnostics-line"><span>状态</span><span>{brokerText[snapshot.broker.state]}</span></div>
              {snapshot.broker.generation !== null ? <div className="diagnostics-line"><span>实例代次</span><span>{snapshot.broker.generation}</span></div> : null}
            </div>
            {snapshot.reconnect ? (
              <div className="diagnostics-section">
                <h3>连接恢复</h3>
                <div className="diagnostics-line"><span>恢复代次</span><span>{snapshot.reconnect.generation}</span></div>
                <div className="diagnostics-line"><span>组件</span><span>{snapshot.reconnect.component}</span></div>
                {snapshot.reconnect.attempts.length ? <div className="diagnostics-attempts">{snapshot.reconnect.attempts.map((attempt) => <span key={attempt.attempt}>第 {attempt.attempt} 次 · {attemptText[attempt.state]}</span>)}</div> : <p className="diagnostics-muted">此故障没有自动重试记录。</p>}
              </div>
            ) : null}
            <div className="diagnostics-section">
              <div className="diagnostics-line"><span>运行密钥</span><span>{snapshot.runtimeKeyPresent ? "已安全保存" : "未保存"}</span></div>
              <div className="diagnostics-line"><span>当前项目</span><span>{snapshot.activeWorkspace ? "已选择" : "未选择"}</span></div>
            </div>
          </>
        )}
        {error ? <p className="diagnostics-error" role="alert">{error}</p> : null}
        {exportedPath ? <p className="diagnostics-exported">已导出到：{exportedPath}</p> : null}
        <div className="dialog-actions diagnostics-actions">
          <button className="secondary" onClick={() => void refresh()}>刷新</button>
          {snapshot?.reconnect ? <button className="secondary" onClick={() => void retry()}>重试连接</button> : null}
          <button className="secondary" onClick={() => void exportReport()}>导出诊断</button>
          <button className="secondary" onClick={onOpenWelcome}>打开欢迎页</button>
          <button className="primary" onClick={onClose}>完成</button>
        </div>
      </section>
    </div>
  );
}
