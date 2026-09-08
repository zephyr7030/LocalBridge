import type { MainProjection } from "../../bridge";
import { overallServiceDetail, overallServiceState } from "../../presentation";
import { ActivityRows } from "./ActivityRows";

/**
 * "能不能用"已经由左上角的标题行回答，这里回答第二个问题：它此刻在干什么。
 * 两者相邻且位于最上方，其余都是上下文。
 */
export function StatusHeadline({ projection }: { projection: MainProjection | null }) {
  const state = overallServiceState(projection);
  const detail = overallServiceDetail(projection);
  return (
    <section className="status-headline">
      {/* 一切正常时不必解释，标题行已经说了；只有出问题时才点名是哪一项。 */}
      {state !== "ready" && detail && <p className="status-detail">{detail}</p>}
      <ActivityRows
        currentActivity={projection?.currentActivity ?? null}
        activityStatus={projection?.activityStatus ?? "unavailable"}
      />
    </section>
  );
}
