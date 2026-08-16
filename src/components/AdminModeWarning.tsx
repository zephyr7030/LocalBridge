import { useEffect, useRef, useState } from "react";

const ADMIN_WARNING_COUNTDOWN_MS = 9000;

export function adminWarningRemainingSeconds(startedAt: number, now: number): number {
  return Math.ceil(Math.max(0, ADMIN_WARNING_COUNTDOWN_MS - (now - startedAt)) / 1000);
}

export function adminWarningCanConfirm(startedAt: number, now: number): boolean {
  return now - startedAt >= ADMIN_WARNING_COUNTDOWN_MS;
}

const ADMIN_WARNING_CONSEQUENCES = [
  "删除或覆盖重要文件",
  "修改系统关键配置",
  "软件或系统无法正常启动",
  "数据永久丢失",
  "安全机制被绕过或关闭",
  "凭据、密钥等敏感信息泄露",
  "恶意程序获得更高权限",
  "系统被破坏，严重时可能需要重装 Windows",
] as const;

export function AdminModeWarning({ onCancel, onConfirm }: { onCancel: () => void; onConfirm: () => void }) {
  const startedAt = useRef(performance.now());
  const [remainingSeconds, setRemainingSeconds] = useState(9);

  useEffect(() => {
    const update = () => {
      setRemainingSeconds(adminWarningRemainingSeconds(startedAt.current, performance.now()));
    };
    update();
    const timer = window.setInterval(update, 100);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onCancel]);

  const confirm = () => {
    if (!adminWarningCanConfirm(startedAt.current, performance.now())) return;
    onConfirm();
  };

  return <div className="dialog-backdrop admin-warning-backdrop" onMouseDown={onCancel}>
    <section className="dialog admin-warning" role="dialog" aria-modal="true" aria-labelledby="admin-warning-title" onMouseDown={(event) => event.stopPropagation()}>
      <h2 id="admin-warning-title">管理员权限确认</h2>
      <p>启用管理员权限后，错误或恶意操作可能导致：</p>
      <ul>{ADMIN_WARNING_CONSEQUENCES.map((item) => <li key={item}>{item}</li>)}</ul>
      <p className="admin-warning-footer">仅在你明确理解操作后果时授权。</p>
      <div className="dialog-actions">
        <button className="secondary" onClick={onCancel}>取消</button>
        <button className="primary admin-warning-confirm" disabled={remainingSeconds > 0} onClick={confirm}>{remainingSeconds > 0 ? `确认${remainingSeconds}` : "确认"}</button>
      </div>
    </section>
  </div>;
}
