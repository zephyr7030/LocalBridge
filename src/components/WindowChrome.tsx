import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent, PropsWithChildren } from "react";

export function WindowChrome({ children }: PropsWithChildren) {
  const startDrag = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging();
  };

  const stopDrag = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
  };

  return (
    <div className="window-chrome">
      <header className="window-titlebar" onMouseDown={startDrag}>
        <span className="window-title">LocalBridge</span>
        <div className="window-controls" aria-label="窗口控制">
          <button
            type="button"
            className="window-control"
            aria-label="最小化"
            onMouseDown={stopDrag}
            onClick={() => void getCurrentWindow().minimize()}
          >
            <span aria-hidden="true">−</span>
          </button>
          <button
            type="button"
            className="window-control window-close"
            aria-label="关闭"
            onMouseDown={stopDrag}
            onClick={() => void getCurrentWindow().close()}
          >
            <span aria-hidden="true">×</span>
          </button>
        </div>
      </header>
      <div className="window-content">{children}</div>
    </div>
  );
}
