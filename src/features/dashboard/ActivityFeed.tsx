import { useEffect, useState } from "react";
import { activityApi, type ActivityEntry } from "../activity/api";
import {
  activityAction,
  activityDuration,
  activityOutcome,
  activityTime,
  activityTone,
  riskText,
} from "../activity/presentation";
import { diagnosticsApi } from "../diagnostics/api";

/**
 * 这台机器上刚刚发生了什么。
 *
 * 这是窗口存在的理由：其余每一项——项目、权限、服务状态——托盘菜单都能
 * 显示，只有"它到底做了什么"必须有地方摊开来看。
 */
export function ActivityFeed() {
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let logRevision = 0;
      let retryDelayMs = 250;
      while (!cancelled) {
        try {
          const next = await activityApi.read();
          if (cancelled) return;
          setEntries(next.entries);
          logRevision = next.logRevision;
          retryDelayMs = 250;
          // 搭诊断面板现成的唤醒通道：每条工具调用都会推高 log revision。
          await diagnosticsApi.waitForChange(0, logRevision);
        } catch {
          if (cancelled) return;
          await new Promise((resolve) => window.setTimeout(resolve, retryDelayMs));
          retryDelayMs = Math.min(retryDelayMs * 2, 5000);
        }
      }
    })();
    return () => { cancelled = true; };
  }, []);

  if (entries === null) {
    return <section className="activity-feed"><p className="activity-empty">正在读取活动记录</p></section>;
  }
  if (entries.length === 0) {
    return (
      <section className="activity-feed">
        <p className="activity-empty">还没有任何操作。ChatGPT 对这台机器做的每一件事都会记在这里。</p>
      </section>
    );
  }

  return (
    <section className="activity-feed" aria-label="活动记录">
      {entries.map((entry) => {
        const outcome = activityOutcome(entry);
        const duration = activityDuration(entry.durationMs);
        return (
          <div
            className={`activity-entry tone-${activityTone(entry)}`}
            key={`${entry.timestampMs}-${entry.action}-${entry.target ?? ""}`}
          >
            <time className="activity-entry-time">{activityTime(entry.timestampMs)}</time>
            <span className="activity-entry-main">
              <span className="activity-entry-action">{activityAction(entry)}</span>
              {entry.target && <span className="activity-entry-target">{entry.target}</span>}
              {entry.risk.map((risk) => (
                <span className="activity-risk" key={risk}>{riskText[risk] ?? risk}</span>
              ))}
            </span>
            <span className="activity-entry-outcome">
              {outcome}
              {entry.exitCode != null && outcome ? ` · ${entry.exitCode}` : null}
              {duration ? ` · ${duration}` : null}
            </span>
          </div>
        );
      })}
    </section>
  );
}
