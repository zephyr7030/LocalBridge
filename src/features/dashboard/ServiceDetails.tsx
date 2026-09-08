import type { MainProjection, ServiceCode } from "../../bridge";
import { overallServiceState, projectionStatusText, serviceText } from "../../presentation";
import { ServiceStatusDot } from "../../components/ServiceStatusDot";

const SERVICES: Array<[keyof MainProjection, string]> = [
  ["localEnvironmentService", "本地运行环境"],
  ["tunnelService", "OpenAI 安全隧道"],
  ["codingService", "编码服务"],
];

/**
 * 三项服务的明细只在出问题时出现。一切正常时标题行那句"已就绪"就是全部
 * 答案，再列三行等于让用户自己去 AND 三个事实。
 */
export function ServiceDetails({ projection }: { projection: MainProjection | null }) {
  if (overallServiceState(projection) === "ready") return null;
  return (
    <section className="service-details" aria-label="服务明细">
      {SERVICES.map(([key, label]) => {
        const service = (projection?.[key] as ServiceCode | null | undefined) ?? null;
        return (
          <div className="row service-detail-row" key={label}>
            <span className="label">{label}</span>
            <span className="value service-value">
              <ServiceStatusDot service={service} />
              <span>
                {service
                  ? serviceText[service]
                  : projection
                    ? projectionStatusText(projection.runtimeStatus)
                    : "正在读取"}
              </span>
            </span>
          </div>
        );
      })}
    </section>
  );
}
