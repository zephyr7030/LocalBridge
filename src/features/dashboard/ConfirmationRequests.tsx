import { useEffect, useState } from "react";
import { ModalSurface } from "../../components/ModalSurface";

import { confirmationApi, type PendingConfirmation } from "../confirmation/api";
import {
  confirmationRemainingText,
  confirmationRiskText,
  confirmationRouteText,
} from "../confirmation/presentation";

/**
 * 等着用户点头的管理员命令。
 *
 * 做成模态而不是内嵌横幅，是因为这个窗口平时是关着的：LocalBridge 的常态
 * 是托盘里的一个图标，用户人在 ChatGPT 那边。内嵌横幅会被放进一个没人在看
 * 的表面，结果是每条高危命令都静静等到超时。窗口由后端的观察者叫到前台，
 * 前端负责把它摆成一件必须回答的事。
 *
 * 一次只问一条。同时摆出几条会诱使人一路点过去，而这里恰恰不希望形成
 * "看都不看就点"的手感。
 */
export function ConfirmationRequests() {
  const [entries, setEntries] = useState<PendingConfirmation[]>([]);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let retryDelayMs = 250;
      let revision = 0;
      while (!cancelled) {
        try {
          const next = await confirmationApi.read();
          if (cancelled) return;
          setEntries(next.entries);
          revision = next.revision;
          retryDelayMs = 250;
          await confirmationApi.waitForChange(revision);
        } catch {
          if (cancelled) return;
          await new Promise((resolve) => window.setTimeout(resolve, retryDelayMs));
          retryDelayMs = Math.min(retryDelayMs * 2, 5000);
        }
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // 只为了让"还剩几分钟"这句话不撒谎；没有待确认时不空转。
  useEffect(() => {
    if (entries.length === 0) return;
    const timer = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, [entries.length]);

  const entry = entries[0];
  if (!entry) return null;

  const decide = (approve: boolean) => {
    setBusy(true);
    void (async () => {
      try {
        await (approve ? confirmationApi.approve(entry.id) : confirmationApi.reject(entry.id));
        setEntries((current) => current.filter((item) => item.id !== entry.id));
      } finally {
        setBusy(false);
      }
    })();
  };

  const remaining = confirmationRemainingText(entry.expiresAtMs, now);
  const waiting = entries.length - 1;

  return (
    // 背景点击不关闭：这不是一个可以顺手划掉的提示。
    <ModalSurface className="confirmation-dialog" backdropClassName="confirmation-backdrop" priority={30} labelledBy="confirmation-title">
        <h2 id="confirmation-title">需要你确认</h2>
        <p className="confirmation-lead">
          ChatGPT 要求以管理员身份运行下面这条命令。它已经被拦下，你不点头就不会执行。
        </p>
        <div className="confirmation-chips">
          <span className="confirmation-route">{confirmationRouteText(entry.route)}</span>
          {confirmationRiskText(entry.risk).map((text) => (
            <span className="activity-risk" key={text}>{text}</span>
          ))}
          {remaining && <span className="confirmation-expiry">{remaining}</span>}
        </div>
        {/* 原文照登。这是用户唯一的判断依据，不能截断、要能选中复制。 */}
        <p className="confirmation-command">{entry.command}</p>
        {entry.workdir && <p className="confirmation-workdir">于 {entry.workdir}</p>}
        {waiting > 0 && (
          <p className="confirmation-more">处理完这条后，还有 {waiting} 条在等待。</p>
        )}
        <div className="dialog-actions">
          <button className="secondary" disabled={busy} onClick={() => decide(false)}>
            拒绝
          </button>
          <button className="primary" disabled={busy} onClick={() => decide(true)}>
            允许这一条
          </button>
        </div>
    </ModalSurface>
  );
}
