import type { ActivityEntry } from "./api";
import {
  activityAction,
  activityDetailDuration,
  activityErrorReason,
  activityOutcome,
  activitySourceText,
} from "./presentation";
import { ModalSurface } from "../../components/ModalSurface";

function DetailRow({ label, value }: { label: string; value: string | number | null | undefined }) {
  if (value == null || value === "") return null;
  return (
    <div className="activity-detail-row">
      <span>{label}</span>
      <span>{value}</span>
    </div>
  );
}

export function ActivityDetail({ entry, onClose }: { entry: ActivityEntry; onClose: () => void }) {
  const outcome = activityOutcome(entry) ?? "状态未知";
  const reason = activityErrorReason(entry);
  const duration = activityDetailDuration(entry.durationMs);

  return (
    <ModalSurface variant="sheet" priority={16} ariaLabel="消息详情" onDismiss={onClose} dismissOnBackdrop>
      <div className="sheet-scroll">
        <h2>消息详情</h2>
        <div className="activity-detail-lead">
          <strong>{activityAction(entry)}</strong>
          {entry.target ? <span>{entry.target}</span> : null}
        </div>
        <div className="activity-detail-grid">
          <DetailRow label="来源" value={activitySourceText(entry.source)} />
          <DetailRow label="状态" value={outcome} />
          <DetailRow label="原因" value={reason} />
          <DetailRow label="时间" value={new Date(entry.timestampMs).toLocaleString("zh-CN", { hour12: false })} />
          <DetailRow label="耗时" value={duration} />
          <DetailRow label="退出码" value={entry.exitCode} />
          <DetailRow label="工作目录" value={entry.workdir} />
          <DetailRow label="错误代码" value={entry.errorCode} />
          <DetailRow label="阶段" value={entry.phase} />
          <DetailRow label="详细原因" value={entry.cause} />
          <DetailRow label="HTTP" value={entry.httpStatus} />
          <DetailRow label="请求 ID" value={entry.requestId} />
          <DetailRow label="连接 ID" value={entry.connectionId} />
          <DetailRow label="尝试" value={entry.attempt} />
        </div>
      </div>
      <div className="dialog-actions sheet-actions">
        <button className="primary" onClick={onClose}>完成</button>
      </div>
    </ModalSurface>
  );
}
