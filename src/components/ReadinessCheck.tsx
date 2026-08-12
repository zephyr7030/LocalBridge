export function ReadinessCheck({ label, ready }: { label: string; ready: boolean }) {
  return (
    <div className="readiness-row">
      <span className={`readiness-dot ${ready ? "ready" : "pending"}`} aria-hidden="true" />
      <span>{label}</span>
      <span className="readiness-state">{ready ? "已通过" : "正在检查"}</span>
    </div>
  );
}
