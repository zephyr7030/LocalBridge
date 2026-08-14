import { useCallback, useEffect, useRef, useState } from "react";
import { APP_NAME } from "./appModel";
import { bridge, type AccessCode, type MainProjection, type ProjectProjection, type ServiceCode } from "./bridge";
import { accessText, formatLastToolAge, lastToolText, privilegeText, serviceText, taskText, uiText } from "./presentation";
import { Onboarding } from "./features/onboarding/Onboarding";
import { onboardingApi, type OnboardingState } from "./features/onboarding/api";
import { Diagnostics } from "./features/diagnostics/Diagnostics";
import { WindowChrome } from "./components/WindowChrome";
import { ServiceStatusDot } from "./components/ServiceStatusDot";
import "./styles.css";

type View = "main" | "settings" | "diagnostics";
function errorText(value: unknown): string { return typeof value === "string" && value.trim() ? value : "操作未完成"; }

export function App() {
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null);
  const [onboardingError, setOnboardingError] = useState(false);
  const [onboardingPreview, setOnboardingPreview] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const frame = window.requestAnimationFrame(() => {
      if (!cancelled) void bridge.uiReady().catch(() => undefined);
    });
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(frame);
    };
  }, []);
  useEffect(() => {
    void onboardingApi.read().then(setOnboarding).catch(() => setOnboardingError(true));
  }, []);
  return (
    <WindowChrome>
      {onboardingError ? <main className="onboarding-loading">无法读取首次设置状态</main>
        : !onboarding ? <main className="onboarding-loading">正在准备 LocalBridge…</main>
          : !onboarding.complete ? <Onboarding initial={onboarding} onComplete={() => setOnboarding({ ...onboarding, complete: true })} />
            : onboardingPreview ? <Onboarding initial={onboarding} previewMode onComplete={() => setOnboardingPreview(false)} />
              : <Dashboard onOpenWelcome={() => setOnboardingPreview(true)} />}
    </WindowChrome>
  );
}

function Dashboard({ onOpenWelcome }: { onOpenWelcome: () => void }) {
  const [projection, setProjection] = useState<MainProjection | null>(null);
  const [view, setView] = useState<View>("main");
  const [error, setError] = useState<string | null>(null);
  const errorTimer = useRef<number | null>(null);
  const [keyValue, setKeyValue] = useState("");
  const [tunnelValue, setTunnelValue] = useState("");
  const [editingTunnel, setEditingTunnel] = useState(false);
  const [editingKey, setEditingKey] = useState(false);
  const [confirmingKeyDelete, setConfirmingKeyDelete] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<ProjectProjection | null>(null);
  const [projectPickerOpen, setProjectPickerOpen] = useState(false);
  const [handledGeneration, setHandledGeneration] = useState<number | null>(null);
  const clearTransientError = useCallback(() => {
    if (errorTimer.current !== null) {
      window.clearTimeout(errorTimer.current);
      errorTimer.current = null;
    }
    setError(null);
  }, []);
  const showTransientError = useCallback((value: unknown) => {
    if (errorTimer.current !== null) {
      window.clearTimeout(errorTimer.current);
    }
    setError(errorText(value));
    errorTimer.current = window.setTimeout(() => {
      setError(null);
      errorTimer.current = null;
    }, 3000);
  }, []);
  const refresh = useCallback(async () => { try { setProjection(await bridge.read()); } catch (value) { showTransientError(value); } }, [showTransientError]);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let revision = 0;
      while (!cancelled) {
        try {
          const next = await bridge.read();
          if (cancelled) return;
          setProjection(next);
          revision = next.projectionRevision;
          await bridge.waitForProjectionChange(revision);
        } catch (value) {
          if (cancelled) return;
          showTransientError(value);
          try { await bridge.waitForProjectionChange(revision); } catch { /* next read retries */ }
        }
      }
    })();
    return () => { cancelled = true; };
  }, [showTransientError]);
  useEffect(() => () => { if (errorTimer.current !== null) window.clearTimeout(errorTimer.current); }, []);
  const run = useCallback(async (action: () => Promise<void>) => { clearTransientError(); try { await action(); await refresh(); } catch (value) { showTransientError(value); } }, [clearTransientError, refresh, showTransientError]);
  useEffect(() => {
    if (view !== "settings" || (projection && !projection.runtimeKeySaved)) setConfirmingKeyDelete(false);
  }, [view, projection?.runtimeKeySaved]);
  const task = projection?.currentTask ?? null;
  const taskActive = task?.state === "running";
  const activeProject = projection?.projects.find((item) => item.active) ?? null;
  const reconnectVisible = Boolean(projection?.reconnect && projection.reconnect.generation !== handledGeneration);
  const privilegeLabel = projection ? privilegeText[projection.privilege] : "未启用";
  const privilegeService: ServiceCode = projection?.privilege === "active" ? "online" : projection?.privilege === "fault" ? "fault" : projection?.privilege === "requested" || projection?.privilege === "awaiting" ? "starting" : "off";
  const chooseAccess = (mode: AccessCode) => void run(() => bridge.setAccess(mode));
  const chooseOtherFolder = () => void run(async () => { const path = await bridge.chooseProjectFolder(); if (path) { await bridge.addProject(path); setProjectPickerOpen(false); } });

  return <main className="shell">
    <header className="topbar"><div className="brand">{APP_NAME}</div><div className="top-actions"><button className="ghost" onClick={() => setView("settings")}>{uiText.settings}</button><button className="ghost" onClick={() => setView("diagnostics")}>{uiText.diagnostics}</button></div></header>
    <section className="card">
      <div className="row"><span className="label">当前项目</span><div className="project-actions"><span className="value">{activeProject?.path ?? "未选择项目"}</span><button className="secondary" onClick={() => setProjectPickerOpen(true)}>{activeProject ? "切换" : "选择项目"}</button></div></div>
      <div className="row"><span className="label">本地运行环境</span><span className="value service-value"><ServiceStatusDot service={projection?.localEnvironmentService ?? null}/><span>{projection ? serviceText[projection.localEnvironmentService] : "正在读取"}</span></span></div>
      <div className="row"><span className="label">OpenAI 安全隧道</span><span className="value service-value"><ServiceStatusDot service={projection?.tunnelService ?? null}/><span>{projection ? serviceText[projection.tunnelService] : "正在读取"}</span></span></div>
      <div className="row"><span className="label">编码服务</span><span className="value service-value"><ServiceStatusDot service={projection?.codingService ?? null}/><span>{projection ? serviceText[projection.codingService] : "正在读取"}</span></span></div>
      <div className="row"><span className="label">管理员权限</span><span className="value service-value"><ServiceStatusDot service={privilegeService}/><span>{privilegeLabel}</span></span></div>
    </section>
    <div className="service-actions" aria-label="服务控制"><button className="secondary service-restart" onClick={() => void run(() => bridge.restartServices())}>重启服务</button><button className="secondary service-stop" onClick={() => void run(() => bridge.stopServices())}>关闭服务</button></div>
    <div className="task-row" aria-live="polite"><span className={`activity-dot ${taskActive ? "active" : ""}`} aria-hidden="true"/><span>{taskText(task)}</span></div>
    {projection?.lastTool && <div className="last-tool-row"><span className="last-tool-label">{lastToolText(projection.lastTool)}</span><span className="last-tool-age">{formatLastToolAge(projection.lastTool.ageMs)}</span></div>}
    {error && <div className="error" role="alert">{error}</div>}
    {view === "settings" && <div className="sheet-backdrop" onMouseDown={() => setView("main")}><section className="sheet" onMouseDown={(event) => event.stopPropagation()}><h2>{uiText.settings}</h2>
      <section className="settings-section"><h3>常规</h3><div className="row"><span>开机启动</span><input type="checkbox" checked={projection?.autoStart ?? false} onChange={(event) => void run(() => bridge.setAutoStart(event.target.checked))}/></div><div className="row"><span>关闭窗口后继续运行</span><input type="checkbox" checked={projection?.closeWindowContinueRunning ?? true} onChange={(event) => void run(() => bridge.setCloseWindowContinueRunning(event.target.checked))}/></div></section>
      <section className="settings-section"><h3>连接</h3>
        <div className="field"><label htmlFor="tunnel-id">Tunnel ID</label>{editingTunnel ? <><input id="tunnel-id" autoComplete="off" value={tunnelValue} onChange={(event) => setTunnelValue(event.target.value)} placeholder="输入 Tunnel ID"/><div className="inline-actions"><button className="primary" disabled={!tunnelValue.trim()} onClick={() => void run(async () => { await bridge.saveTunnelId(tunnelValue.trim()); setTunnelValue(""); setEditingTunnel(false); })}>保存</button><button className="secondary" onClick={() => { setTunnelValue(""); setEditingTunnel(false); }}>取消</button></div></> : <div className="settings-summary"><span>{projection?.tunnelId ?? "未保存"}</span><button className="secondary settings-replace" onClick={() => { setTunnelValue(projection?.tunnelId ?? ""); setEditingTunnel(true); }}>更换</button></div>}</div>
        <div className="field"><label htmlFor="runtime-key">Runtime API Key</label>{editingKey ? <><input id="runtime-key" type="password" autoComplete="off" value={keyValue} onChange={(event) => setKeyValue(event.target.value)} placeholder="输入新的 Runtime API Key"/><div className="inline-actions"><button className="primary" disabled={!keyValue.trim()} onClick={() => void run(async () => { await bridge.saveKey(keyValue); setKeyValue(""); setEditingKey(false); })}>保存</button><button className="secondary" onClick={() => { setKeyValue(""); setEditingKey(false); }}>取消</button></div></> : confirmingKeyDelete && projection?.runtimeKeySaved ? <div className="settings-summary settings-delete-confirm"><span>请确认从windows安全凭据中删除？</span><button className="secondary settings-delete-cancel" onClick={() => setConfirmingKeyDelete(false)}>取消</button><button className="secondary settings-confirm-delete" onClick={() => void run(async () => { await bridge.clearKey(); setConfirmingKeyDelete(false); })}>确认</button></div> : <div className="settings-summary"><span>{projection?.runtimeKeySaved ? "已保存" : "未保存"}</span>{projection?.runtimeKeySaved && <button className="secondary settings-clear" onClick={() => setConfirmingKeyDelete(true)}>清除</button>}<button className="secondary settings-replace" onClick={() => { setConfirmingKeyDelete(false); setKeyValue(""); setEditingKey(true); }}>更换</button></div>}</div>
      </section>
      <section className="settings-section"><h3>权限</h3><div className="access-grid">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} className={`choice ${mode === "admin" ? "admin-choice" : ""} ${projection?.permission === mode ? "selected" : ""}`} onClick={() => chooseAccess(mode)}>{accessText[mode]}</button>)}</div></section>
      <div className="dialog-actions"><button className="secondary" onClick={onOpenWelcome}>打开欢迎页</button><button className="primary" onClick={() => setView("main")}>完成</button></div>
    </section></div>}
    {view === "diagnostics" && <Diagnostics onClose={() => setView("main")} />}
    {projectPickerOpen && <div className="sheet-backdrop" onMouseDown={() => setProjectPickerOpen(false)}><section className="sheet" onMouseDown={(event) => event.stopPropagation()}><h2>切换项目</h2><div className="project-list">{projection?.projects.map((item) => <div className="project-item" key={item.id}><button className="ghost project-select" disabled={item.active} onClick={() => void run(async () => { await bridge.selectProject(item.id); setProjectPickerOpen(false); })}><span className="project-path">{item.path}</span>{item.active ? <span className="project-current">当前</span> : null}</button><button className="secondary" onClick={() => { if (item.active) { setProjectPickerOpen(false); setRemoveTarget(item); } else { void run(() => bridge.removeProject(item.id)); } }}>移除</button></div>)}</div><div className="dialog-actions"><button className="secondary" onClick={chooseOtherFolder}>选择其他文件夹</button><button className="primary" onClick={() => setProjectPickerOpen(false)}>完成</button></div></section></div>}
    {removeTarget && <div className="dialog-backdrop"><section className="dialog"><h2>移除当前项目</h2><p>从 LocalBridge 移除此项目？<br/>不会删除项目文件。</p><div className="dialog-actions"><button className="secondary" onClick={() => setRemoveTarget(null)}>取消</button><button className="primary" onClick={() => void run(async () => { await bridge.removeProject(removeTarget.id); setRemoveTarget(null); })}>移除</button></div></section></div>}
    {reconnectVisible && projection?.reconnect && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>连接失败</h2><p>已自动重试 5 次。</p><div className="dialog-actions"><button className="secondary" onClick={() => void run(async () => { await bridge.retry(); setHandledGeneration(projection.reconnect?.generation ?? null); })}>重试</button><button className="primary" onClick={() => { setHandledGeneration(projection.reconnect?.generation ?? null); setView("diagnostics"); }}>查看诊断</button></div></section></div>}
  </main>;
}
