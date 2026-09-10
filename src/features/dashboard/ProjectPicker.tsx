import { bridge, type MainProjection, type ProjectProjection } from "../../bridge";
import { ModalSurface } from "../../components/ModalSurface";
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
    <ModalSurface variant="sheet" labelledBy="project-picker-title" onDismiss={onClose} dismissOnBackdrop>
      <div className="sheet-scroll">
        <h2 id="project-picker-title">切换项目</h2>
        <div className="project-list">
          {projection?.projects?.map((item) => (
            <div className="project-item" key={item.id}>
              <div className="project-path-area">
                <span className="project-path" title={item.path}>{item.path}</span>
              </div>
              <div className="project-item-actions">
                <button
                  className="secondary"
                  disabled={item.active}
                  onClick={() =>
                    void run(async () => {
                      await bridge.selectProject(item.id);
                      onClose();
                    })
                  }
                >
                  {item.active ? "当前" : "切换"}
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
            </div>
          ))}
        </div>
      </div>
      <div className="dialog-actions sheet-actions">
          <button className="secondary" onClick={chooseOtherFolder}>
            选择其他文件夹
          </button>
          <button className="primary" onClick={onClose}>
            完成
          </button>
        </div>
    </ModalSurface>
  );
}
