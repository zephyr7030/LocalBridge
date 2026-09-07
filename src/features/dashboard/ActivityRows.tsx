import type { CurrentActivityProjection, LastActivityProjection, ProjectionStatusCode } from "../../bridge";
import {
  currentActivityDetail,
  currentActivityElapsed,
  currentActivityText,
  formatLastToolAge,
  lastActivityAction,
  lastActivityOutcome,
} from "../../presentation";

export function ActivityRows({
  currentActivity,
  lastActivity,
  activityStatus,
}: {
  currentActivity: CurrentActivityProjection | null;
  lastActivity: LastActivityProjection | null;
  activityStatus: ProjectionStatusCode;
}) {
  const taskState = currentActivity?.state ?? (activityStatus === "ready" ? "idle" : "unavailable");
  const detail = currentActivityDetail(currentActivity);
  const elapsed = currentActivityElapsed(currentActivity);
  return (
    <>
      <div className="task-row" aria-live="polite">
        <span className={`activity-dot task-${taskState}`} aria-hidden="true" />
        <span className="activity-row-main">
          <span className="activity-action">{currentActivityText(currentActivity, activityStatus)}</span>
          {detail && <span className="activity-summary">{detail}</span>}
        </span>
        {elapsed && <span className="activity-elapsed">{elapsed}</span>}
      </div>
      {lastActivity ? (
        <div className="last-tool-row">
          <span className="activity-dot-spacer" aria-hidden="true" />
          <span className={`last-activity-left outcome-${lastActivity.outcome}`}>
            <span className="activity-action">上次执行：{lastActivityAction(lastActivity)}</span>
            {lastActivity.summary && <span className="activity-summary">{lastActivity.summary}</span>}
            <span className="activity-outcome">{lastActivityOutcome(lastActivity)}</span>
          </span>
          <span className="last-tool-age">
            {formatLastToolAge(Math.max(0, Date.now() - lastActivity.completedAtMs))}
          </span>
        </div>
      ) : null}
    </>
  );
}
