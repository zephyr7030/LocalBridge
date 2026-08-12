import { useCallback, useEffect, useMemo, useState } from "react";
import { APP_NAME } from "./appModel";
import { bridge, type AccessCode, type MainProjection, type ProjectProjection } from "./bridge";
import { accessText, privilegeText, serviceText, taskText, uiText } from "./presentation";
import { Onboarding } from "./features/onboarding/Onboarding";
import { onboardingApi, type OnboardingState } from "./features/onboarding/api";
import "./styles.css";

type View = "main" | "settings" | "diagnostics";
function errorText(value: unknown): string { return typeof value === "string" && value.trim() ? value : "操作未完成"; }

export function App() {
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null);
  const [onboardingError, setOnboardingError] = useState(false);
  useEffect(() => {
    void onboardingApi.read().then(setOnboarding).catch(() => setOnboardingError(true));
  }, []);
  if (onboardingError) return <main className="onboarding-loading">无法读取首次设置状态</main>;
  if (!onboarding) return <main className="onboarding-loading">正在准备 LocalBridge…</main>;
  if (!onboarding.complete) return <Onboarding initial={onboarding} onComplete={() => setOnboarding({ ...onboarding, complete: true })} />;
  return <Dashboard />;
}

function Dashboard() {
  const [projection, setProjection] = useState<MainProjection | null>(null);
  const [view, setView] = useState<View>("main");
  const [error, setError] = useState<string | null>(null);
  const [keyValue, setKeyValue] = useState("");
  const [keySaved, setKeySaved] = useState(false);
  const [pathEditor, setPathEditor] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [removeTarget, setRemoveTarget] = useState<ProjectProjection | null>(null);
  const [handledGeneration, setHandledGeneration] = useState<number | null>(null);
  const refresh = useCallback(async () => { try { setProjection(await bridge.read()); } catch (value) { setError(errorText(value)); } }, []);
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 1200); return () => window.clearInterval(timer); }, [refresh]);
  const run = useCallback(async (action: () => Promise<void>) => { setError(null); try { await action(); await refresh(); } catch (value) { setError(errorText(value)); } }, [refresh]);
  const task = projection?.currentTask ?? null;
  const taskActive = task?.state === "running";
  const activeProject = projection?.projects.find((item) => item.active) ?? null;
  const reconnectVisible = Boolean(projection?.reconnect && projection.reconnect.generation !== handledGeneration);
  const privilegeLabel = projection ? privilegeText[projection.privilege] : "未启用";
  const statusClass = projection?.privilege === "fault" ? "status-bad" : projection?.privilege === "active" ? "status-good" : "";
  const adminAction = useMemo(() => {
    if (!projection || projection.permission !== "admin") return null;
    if (projection.privilege === "active") return <button className="secondary" onClick={() => void run(bridge.disableAdmin)}>关闭管理员权限</button>;
    if (projection.privilege === "awaiting") return null;
    return <button className="secondary" onClick={() => void run(bridge.enableAdmin)}>启用管理员权限</button>;
  }, [projection, run]);
  const chooseAccess = (mode: AccessCode) => void run(() => bridge.setAccess(mode));
  const confirmRemove = async (item: ProjectProjection) => { if (item.active) { setRemoveTarget(item); return; } await run(() => bridge.removeProject(item.id)); };

  return <main className="shell">
    <header className="topbar"><div className="brand">{APP_NAME}</div><div className="top-actions"><button className="ghost" onClick={() => setView("settings")}>{uiText.settings}</button><button className="ghost" onClick={() => setView("diagnostics")}>{uiText.diagnostics}</button></div></header>
    <section className="card">
      <div className="row"><span className="label">当前项目</span><div className="project-actions">{projection?.projects.length ? <select value={activeProject?.id ?? ""} onChange={(event) => event.target.value && void run(() => bridge.selectProject(event.target.value))} aria-label="选择项目">{!activeProject && <option value="">选择项目</option>}{projection.projects.map((item) => <option key={item.id} value={item.id}>{item.path}</option>)}</select> : <span className="value">未选择项目</span>}<button className="secondary" onClick={() => setPathEditor(true)}>选择其他文件夹</button></div></div>
      <div className="row"><span className="label">OpenAI 安全隧道</span><span className="value">{projection ? serviceText[projection.tunnelService] : "正在读取"}</span></div>
      <div className="row"><span className="label">编码服务</span><span className="value">{projection ? serviceText[projection.codingService] : "正在读取"}</span></div>
      <div className="row"><span className="label">管理员权限</span><span className={`value ${statusClass}`}>{privilegeLabel}</span></div>
    </section>
    <section className="card"><span className="label">权限模式</span><div className="access-grid">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} className={`choice ${projection?.permission === mode ? "selected" : ""}`} onClick={() => chooseAccess(mode)}>{accessText[mode]}</button>)}</div>{adminAction && <div className="inline-actions" style={{ marginTop: 12 }}>{adminAction}</div>}</section>
    <div className="task-row" aria-live="polite"><span className={`activity-dot ${taskActive ? "active" : ""}`} aria-hidden="true"/><span>{taskText(task)}</span></div>
    {error && <div className="error" role="alert">{error}</div>}
    {view === "settings" && <div className="sheet-backdrop" onMouseDown={() => setView("main")}><section className="sheet" onMouseDown={(event) => event.stopPropagation()}><h2>{uiText.settings}</h2><div className="row"><span>开机启动</span><input type="checkbox" checked={projection?.autoStart ?? false} onChange={(event) => void run(() => bridge.setAutoStart(event.target.checked))}/></div><div className="field"><label htmlFor="runtime-key">运行密钥</label><input id="runtime-key" type="password" autoComplete="off" value={keyValue} onChange={(event) => { setKeyValue(event.target.value); setKeySaved(false); }} placeholder={projection?.runtimeKeySaved ? "已保存" : "输入运行密钥"}/><div className="inline-actions"><button className="primary" disabled={!keyValue} onClick={() => void run(async () => { await bridge.saveKey(keyValue); setKeyValue(""); setKeySaved(true); })}>保存</button>{projection?.runtimeKeySaved && <button className="secondary" onClick={() => void run(bridge.deleteKey)}>删除</button>}{keySaved && <span className="saved">已保存</span>}</div></div><div className="field"><span>已保存项目</span><div className="project-list">{projection?.projects.map((item) => <div className="project-item" key={item.id}><span className="project-path">{item.path}</span><button className="secondary" onClick={() => void confirmRemove(item)}>移除</button></div>)}</div></div><div className="dialog-actions"><button className="primary" onClick={() => setView("main")}>完成</button></div></section></div>}
    {view === "diagnostics" && <div className="sheet-backdrop" onMouseDown={() => setView("main")}><section className="sheet" onMouseDown={(event) => event.stopPropagation()}><h2>{uiText.diagnostics}</h2><div className="row"><span>安全隧道</span><span>{projection ? serviceText[projection.tunnelService] : "正在读取"}</span></div><div className="row"><span>编码服务</span><span>{projection ? serviceText[projection.codingService] : "正在读取"}</span></div><div className="row"><span>管理员权限</span><span>{privilegeLabel}</span></div><div className="row"><span>当前项目</span><span className="value">{projection?.currentProject ?? "未选择项目"}</span></div><div className="row"><span>运行密钥</span><span>{projection?.runtimeKeySaved ? "已保存" : "未保存"}</span></div><div className="dialog-actions"><button className="primary" onClick={() => setView("main")}>完成</button></div></section></div>}
    {pathEditor && <div className="dialog-backdrop"><section className="dialog"><h2>选择其他文件夹</h2><div className="field"><label htmlFor="project-path">文件夹路径</label><input id="project-path" type="text" value={newPath} onChange={(event) => setNewPath(event.target.value)}/></div><div className="dialog-actions"><button className="secondary" onClick={() => { setPathEditor(false); setNewPath(""); }}>取消</button><button className="primary" disabled={!newPath.trim()} onClick={() => void run(async () => { await bridge.addProject(newPath.trim()); setPathEditor(false); setNewPath(""); })}>使用此文件夹</button></div></section></div>}
    {removeTarget && <div className="dialog-backdrop"><section className="dialog"><h2>移除当前项目</h2><p>从 LocalBridge 移除此项目？<br/>不会删除项目文件。</p><div className="dialog-actions"><button className="secondary" onClick={() => setRemoveTarget(null)}>取消</button><button className="primary" onClick={() => void run(async () => { await bridge.removeProject(removeTarget.id); setRemoveTarget(null); })}>移除</button></div></section></div>}
    {reconnectVisible && projection?.reconnect && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>连接失败</h2><p>已自动重试 5 次。</p><div className="dialog-actions"><button className="secondary" onClick={() => void run(async () => { await bridge.retry(); setHandledGeneration(projection.reconnect?.generation ?? null); })}>重试</button><button className="primary" onClick={() => { setHandledGeneration(projection.reconnect?.generation ?? null); setView("diagnostics"); }}>查看诊断</button></div></section></div>}
  </main>;
}
