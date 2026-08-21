import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ClipboardEvent, type FormEvent } from "react";
import { WizardFrame } from "../../components/WizardFrame";
import { ReadinessCheck } from "../../components/ReadinessCheck";
import { AdminModeWarning } from "../../components/AdminModeWarning";
import { bridge, type AccessCode, type MainProjection, uiErrorMessage } from "../../bridge";
import { accessText } from "../../presentation";
import { onboardingApi, type OnboardingState } from "./api";
import "./onboarding.css";

const KEY_HINT = "Runtime API Key 仅保存在 Windows 安全凭据中。";

type Screen4CopyKey = "name" | "tunnel";

const messageFrom = (value: unknown, fallback: string) => uiErrorMessage(value, fallback);

export function Onboarding({ initial, onComplete, previewMode = false }: { initial: OnboardingState; onComplete: () => void; previewMode?: boolean }) {
  const viteDev = (import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV === true;
  const permissionGeometryE2e = viteDev
    && typeof window !== "undefined"
    && new URLSearchParams(window.location.search).get("lb016-e2e") === "permission-geometry";
  const [step, setStep] = useState(permissionGeometryE2e ? 3 : 1);
  const [state, setState] = useState(initial);
  const [main, setMain] = useState<MainProjection | null>(null);
  const [tunnelId, setTunnelId] = useState(initial.tunnelId ?? "");
  const [runtimeKey, setRuntimeKey] = useState("");
  const [runtimeKeyEditing, setRuntimeKeyEditing] = useState(!initial.runtimeKeySaved);
  const [runtimeKeyFocused, setRuntimeKeyFocused] = useState(false);
  const [permissionGeometryFailed, setPermissionGeometryFailed] = useState(false);
  const [selectedFolder, setSelectedFolder] = useState("");
  const [rememberedProject, setRememberedProject] = useState("");
  const [permission, setPermission] = useState<AccessCode>("edit");
  const [copiedRows, setCopiedRows] = useState<Record<Screen4CopyKey, boolean>>({ name: false, tunnel: false });
  const copyTimers = useRef<Record<Screen4CopyKey, number | null>>({ name: null, tunnel: null });
  const [preparingProject, setPreparingProject] = useState(false);
  const [adminWarningOpen, setAdminWarningOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const savedRuntimeKeyMask = state.runtimeKeySaved && state.runtimeKeyLength
    ? "*".repeat(state.runtimeKeyLength)
    : "";
  const runtimeKeyDisplayValue = runtimeKeyEditing
    ? runtimeKey
    : runtimeKeyFocused
      ? savedRuntimeKeyMask
      : "";
  const allGreen = main !== null
    && main.localEnvironmentService === "online"
    && main.codingService === "online"
    && main.tunnelService === "online";

  useEffect(() => () => {
    for (const timer of Object.values(copyTimers.current)) {
      if (timer !== null) window.clearTimeout(timer);
    }
  }, []);

  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(() => setError(null), 3000);
    return () => window.clearTimeout(timer);
  }, [error]);

  useLayoutEffect(() => {
    if (!permissionGeometryE2e || step !== 3) return;
    const permissionButtons = Array.from(document.querySelectorAll<HTMLButtonElement>(".onboarding-permission"));
    const lineBoxesInside = (button: HTMLButtonElement, element: Element | null) => {
      if (!element) return false;
      const buttonRect = button.getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(element);
      const boxes = Array.from(range.getClientRects());
      return boxes.length > 0 && boxes.every((box) => box.width > 0 && box.height > 0
        && box.top >= buttonRect.top - 0.5 && box.bottom <= buttonRect.bottom + 0.5
        && box.left >= buttonRect.left - 0.5 && box.right <= buttonRect.right + 0.5);
    };
    const rectangles = permissionButtons.map((button) => button.getBoundingClientRect());
    const widths = rectangles.map((rect) => rect.width);
    const heights = rectangles.map((rect) => rect.height);
    const gaps = rectangles.slice(1).map((rect, index) => rect.left - rectangles[index].right);
    const geometryPass = window.innerWidth === 780
      && permissionButtons.length === 3
      && Math.max(...widths) - Math.min(...widths) <= 0.5
      && Math.max(...heights) - Math.min(...heights) <= 0.5
      && gaps.length === 2
      && Math.abs(gaps[0] - gaps[1]) <= 0.5
      && permissionButtons.every((button) => button.getBoundingClientRect().width > 0
        && button.getBoundingClientRect().height > 0
        && lineBoxesInside(button, button.querySelector("strong"))
        && lineBoxesInside(button, button.querySelector("small")));
    if (!geometryPass) setPermissionGeometryFailed(true);
  }, [permissionGeometryE2e, step]);

  useEffect(() => {
    void bridge.read().then((projection) => {
      setMain(projection);
      setPermission(projection.permission);
      const active = projection.projects.find((item) => item.active);
      if (active) setRememberedProject(active.id);
    }).catch(() => setError("无法读取项目设置"));
  }, []);

  useEffect(() => {
    if (step !== 4 && step !== 5) return;
    let cancelled = false;
    void (async () => {
      let revision = 0;
      while (!cancelled) {
        try {
          const projection = await bridge.read();
          if (cancelled) return;
          setMain(projection);
          revision = projection.projectionRevision;
          await bridge.waitForProjectionChange(revision);
        } catch {
          if (cancelled) return;
          setError("无法读取运行状态");
          try {
            await bridge.waitForProjectionChange(revision);
          } catch {
            /* next backend wake/read retries */
          }
        }
      }
    })();
    return () => { cancelled = true; };
  }, [step]);

  const chosenProject = useMemo(() => main?.projects.find((item) => item.id === rememberedProject) ?? null, [main, rememberedProject]);

  const saveConnection = async () => {
    setError(null);
    try {
      await onboardingApi.saveConnection(tunnelId.trim(), runtimeKeyEditing ? runtimeKey : "");
      setRuntimeKey("");
      const nextState = await onboardingApi.read();
      setState(nextState);
      setTunnelId(nextState.tunnelId ?? "");
      setRuntimeKeyEditing(!nextState.runtimeKeySaved);
      setRuntimeKeyFocused(false);
      setStep(3);
    } catch (value) {
      setError(messageFrom(value, "OpenAI 连接设置未保存"));
    }
  };

  const startRuntimeKeyReplacement = (value = "") => {
    setRuntimeKeyEditing(true);
    setRuntimeKey(value);
  };

  const handleRuntimeKeyBeforeInput = (event: FormEvent<HTMLInputElement>) => {
    if (runtimeKeyEditing || !state.runtimeKeySaved) return;
    const input = event.nativeEvent as InputEvent;
    if (input.inputType.startsWith("delete")) {
      event.preventDefault();
      startRuntimeKeyReplacement();
    } else if (input.inputType.startsWith("insert") && input.data) {
      event.preventDefault();
      startRuntimeKeyReplacement(input.data);
    }
  };

  const handleRuntimeKeyPaste = (event: ClipboardEvent<HTMLInputElement>) => {
    if (runtimeKeyEditing || !state.runtimeKeySaved) return;
    event.preventDefault();
    startRuntimeKeyReplacement(event.clipboardData.getData("text"));
  };

  const chooseFolder = async () => {
    setError(null);
    try {
      const folder = await onboardingApi.chooseWorkspaceFolder();
      if (!folder) return;
      setSelectedFolder(folder);
      setRememberedProject("");
    } catch (value) {
      setError(messageFrom(value, "无法选择项目文件夹"));
    }
  };

  const saveProjectAndPermission = async () => {
    if (preparingProject) return;
    setError(null);
    setPreparingProject(true);
    try {
      const readyState = await onboardingApi.prepareProject(permission, rememberedProject || null, selectedFolder || null);
      setState(readyState);
      setMain(await bridge.read());
      setSelectedFolder("");
      setStep(4);
    } catch (value) {
      setError(messageFrom(value, "本地服务启动失败，请重试"));
    } finally {
      setPreparingProject(false);
    }
  };

  const applyPermission = async (mode: AccessCode) => {
    setError(null);
    try {
      await bridge.setAccess(mode);
      setPermission(mode);
      setMain(await bridge.read());
    } catch (value) {
      setError(messageFrom(value, "权限模式未更新"));
    }
  };

  const choosePermission = async (mode: AccessCode) => {
    if (mode === "admin" && main?.privilege !== "active") {
      setAdminWarningOpen(true);
      return;
    }
    await applyPermission(mode);
  };

  const copyScreen4Value = async (key: Screen4CopyKey, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      const previousTimer = copyTimers.current[key];
      if (previousTimer !== null) window.clearTimeout(previousTimer);
      setCopiedRows((current) => ({ ...current, [key]: true }));
      copyTimers.current[key] = window.setTimeout(() => {
        setCopiedRows((current) => ({ ...current, [key]: false }));
        copyTimers.current[key] = null;
      }, 3000);
    } catch {
      setError("复制失败，请重试");
    }
  };

  const finish = async () => {
    if (!allGreen) return;
    setError(null);
    try {
      if (!previewMode) await onboardingApi.complete();
      onComplete();
    } catch (value) {
      setError(messageFrom(value, "设置尚未完成"));
    }
  };

  if (permissionGeometryFailed) return <main className="onboarding-geometry-failed" data-lb016-permission-geometry="fail" />;

  if (step === 1) return (
    <WizardFrame step={1} title="简单设置 即可开始" footer={<button className="primary" onClick={() => setStep(2)}>开始</button>}>
      <p className="onboarding-copy">LocalBridge是链接ChatGPT与本地代码的工具</p>
    </WizardFrame>
  );

  if (step === 2) return (
    <WizardFrame step={2} title="OpenAI 设置" footer={<><button className="secondary" onClick={() => setStep(1)}>返回</button><button className="primary" disabled={!tunnelId.trim() || (runtimeKeyEditing ? !runtimeKey : !state.runtimeKeySaved)} onClick={() => void saveConnection()}>继续</button></>}>
      <div className="onboarding-field"><label htmlFor="tunnel-id">Tunnel ID</label><input id="tunnel-id" value={tunnelId} onChange={(event) => setTunnelId(event.target.value)} placeholder="tunnel_…" autoComplete="off" /></div>
      <div className="onboarding-link-row"><button className="secondary" onClick={() => void onboardingApi.openTunnelSettings().catch(() => setError("无法打开 Tunnel ID 设置"))}>打开 Tunnel ID 设置</button></div>
      <div className="onboarding-field"><label htmlFor="runtime-key-onboarding">Runtime API Key</label><input id="runtime-key-onboarding" type="password" value={runtimeKeyDisplayValue} onFocus={() => setRuntimeKeyFocused(true)} onBlur={() => setRuntimeKeyFocused(false)} onBeforeInput={handleRuntimeKeyBeforeInput} onPaste={handleRuntimeKeyPaste} onChange={(event) => { if (runtimeKeyEditing) setRuntimeKey(event.target.value); }} placeholder={state.runtimeKeySaved ? "已安全保存至windows安全凭据" : "输入 Runtime API Key"} autoComplete="off" /></div>
      <div className="onboarding-link-row"><button className="secondary" onClick={() => void onboardingApi.openApiKeys().catch(() => setError("无法打开 Runtime API Key 设置"))}>打开 Runtime API Key 设置</button></div>
      <p className="onboarding-hint">{KEY_HINT}</p>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 3) return (<>
    <WizardFrame step={3} title="项目与权限" footer={<><button className="secondary" disabled={preparingProject} onClick={() => setStep(2)}>返回</button><button className="primary" disabled={preparingProject || (!selectedFolder && !rememberedProject)} onClick={() => void saveProjectAndPermission()}>{preparingProject ? "正在启动…" : "继续"}</button></>}>
      {main?.projects.length ? <div className="onboarding-field"><label htmlFor="remembered-project">已保存项目</label><select id="remembered-project" value={rememberedProject} onChange={(event) => { setRememberedProject(event.target.value); setSelectedFolder(""); }}><option value="">选择项目</option>{main.projects.map((item) => <option key={item.id} value={item.id}>{item.path}</option>)}</select></div> : null}
      <div className="onboarding-folder-row"><button className="secondary" onClick={() => void chooseFolder()}>选择项目文件夹</button><span className="onboarding-selected-folder">{selectedFolder || chosenProject?.path || "尚未选择"}</span></div>
      <div className="onboarding-permissions">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} disabled={preparingProject} className={`choice onboarding-permission ${mode === "admin" ? "admin-choice" : ""} ${permission === mode ? "selected" : ""}`} aria-pressed={permission === mode} onClick={() => void choosePermission(mode)}><strong>{accessText[mode]}</strong><small>{mode === "edit" ? "读取、搜索和修改项目文件" : mode === "full" ? "允许运行测试、编译和其他本地命令" : "在完整模式基础上允许显式管理员操作"}</small></button>)}</div>
      {permission === "admin" ? <p className="onboarding-hint">管理员模式会请求 Windows 管理员授权。</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
    {adminWarningOpen && <AdminModeWarning onCancel={() => setAdminWarningOpen(false)} onConfirm={() => { setAdminWarningOpen(false); void applyPermission("admin"); }} />}
  </>);

  if (step === 4) return (
    <WizardFrame step={4} title="创建自定义插件" footer={<><button className="secondary" onClick={() => setStep(3)}>返回</button><button className="primary" disabled={!allGreen} onClick={() => { setError(null); setStep(5); }}>继续</button></>}>
      <p className="onboarding-copy">在插件设置页面最底端，打开“开发者模式”</p>
      <div className="onboarding-plugin-top-action"><button className="secondary" onClick={() => void onboardingApi.openPluginsSettings().catch(() => setError("无法打开 ChatGPT插件设置"))}>打开 ChatGPT插件设置</button></div>
      <p className="onboarding-hint">打开新建插件页后，选择隧道并选择刚刚添加的Tunel，创建插件</p>
      <div className="onboarding-plugin-management"><button className="secondary" disabled={!allGreen} onClick={() => void onboardingApi.openConnectorSettings().catch(() => setError("无法打开新建插件页"))}>打开新建插件页</button></div>
      <div className="onboarding-plugin-info">
        <div className="onboarding-info-row"><span className="onboarding-info-label">名称</span><span className="onboarding-info-value">Local Bridge</span><button className={`secondary onboarding-copy-action ${copiedRows.name ? "copied" : ""}`} onClick={() => void copyScreen4Value("name", "Local Bridge")}>{copiedRows.name ? "已复制" : "复制"}</button></div>
        <div className="onboarding-info-row"><span className="onboarding-info-label">Tunnel ID</span><span className="onboarding-info-value">{state.tunnelId ?? "—"}</span><button className={`secondary onboarding-copy-action ${copiedRows.tunnel ? "copied" : ""}`} disabled={!state.tunnelId} onClick={() => state.tunnelId && void copyScreen4Value("tunnel", state.tunnelId)}>{copiedRows.tunnel ? "已复制" : "复制"}</button></div>
      </div>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 5) return (
    <WizardFrame step={5} title="启动检查" footer={<><button className="secondary" onClick={() => setStep(4)}>返回</button><button className="primary" disabled={!allGreen} onClick={() => void finish()}>确定</button></>}>
      <div className="readiness-list">
        <ReadinessCheck label="本地运行环境" service={main?.localEnvironmentService ?? null} />
        <ReadinessCheck label="编码服务" service={main?.codingService ?? null} />
        <ReadinessCheck label="OpenAI Tunnel" service={main?.tunnelService ?? null} />
      </div>
      {allGreen ? <p className="onboarding-success">配置完成，在插件中选择刚刚添加的Local Bridge试试吧</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  return null;
}
