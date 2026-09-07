import { bridge, type MainProjection, type ProjectProjection } from "../../bridge";
import type { DashboardRun } from "./useDashboardProjection";

export function ProjectPicker({
  projection,
  run,
  onClose,
  onConfirmRemoveActive,
}: {
  projection: MainProjection | null;
  run: DashboardRun;
  onClose: () => void;
  onConfirmRemoveActive: (project: ProjectProjection) => void;
}) {
  const chooseOtherFolder = () =>
    void run(async () => {
      const path = await bridge.chooseProjectFolder();
      if (path) {
        await bridge.addProject(path);
        onClose();
      }
    });
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <section className="sheet" onMouseDown={(event) => event.stopPropagation()}>
        <h2>切换项目</h2>
        <div className="project-list">
          {projection?.projects?.map((item) => (
            <div className="project-item" key={item.id}>
              <button
                className="ghost project-select"
                disabled={item.active}
                onClick={() =>
                  void run(async () => {
                    await bridge.selectProject(item.id);
                    onClose();
                  })
                }
              >
                <span className="project-path">{item.path}</span>
                {item.active ? <span className="project-current">当前</span> : null}
              </button>
              <button
                className="secondary"
                onClick={() => {
                  if (item.active) {
                    onClose();
                    onConfirmRemoveActive(item);
                  } else {
                    void run(() => bridge.removeProject(item.id));
                  }
                }}
              >
                移除
              </button>
            </div>
          ))}
        </div>
        <div className="dialog-actions">
          <button className="secondary" onClick={chooseOtherFolder}>
            选择其他文件夹
          </button>
          <button className="primary" onClick={onClose}>
            完成
          </button>
        </div>
      </section>
    </div>
  );
}
