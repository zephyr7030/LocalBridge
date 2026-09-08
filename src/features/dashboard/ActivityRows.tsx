import type { CurrentActivityProjection, ProjectionStatusCode } from "../../bridge";
import { currentActivityDetail, currentActivityElapsed, currentActivityText } from "../../presentation";

/**
 * 此刻在干什么，一行。
 *
 * 这里曾经还有一行"上次执行 ⋯"。下面的活动流上线之后它就是同一份数据的
 * 第二次陈述，而且陈述得更差——只有一条、没有时间戳、没有耗时。
 */
export function ActivityRows({
  currentActivity,
  activityStatus,
}: {
  currentActivity: CurrentActivityProjection | null;
  activityStatus: ProjectionStatusCode;
}) {
  const taskState = currentActivity?.state ?? (activityStatus === "ready" ? "idle" : "unavailable");
  const detail = currentActivityDetail(currentActivity);
  const elapsed = currentActivityElapsed(currentActivity);
  return (
    <div className="task-row" aria-live="polite">
      <span className="activity-row-main">
        <span className={`activity-dot task-${taskState}`} aria-hidden="true" />
        <span className="activity-action">{currentActivityText(currentActivity, activityStatus)}</span>
        {detail && <span className="activity-summary">{detail}</span>}
      </span>
      {elapsed && <span className="activity-elapsed">{elapsed}</span>}
    </div>
  );
}
