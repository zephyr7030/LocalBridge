import { useEffect, useMemo, useRef, useState } from "react";
import { WizardFrame } from "../../components/WizardFrame";
import { ReadinessCheck } from "../../components/ReadinessCheck";
import { bridge, type AccessCode, type MainProjection } from "../../bridge";
import { accessText } from "../../presentation";
import { onboardingApi, type OnboardingState } from "./api";
import "./onboarding.css";

const KEY_HINT = "Runtime API Key 仅保存在 Windows 安全凭据中，不会写入配置文件、日志、命令行或浏览器存储。";
const READY_POLL_INTERVAL_MS = 1000;
const READY_POLL_ATTEMPTS = 60;

type Screen4CopyKey = "name" | "tunnel";

const messageFrom = (value: unknown, fallback: string) =>
  typeof value === "string" ? value : value instanceof Error ? value.message : fallback;

export function Onboarding({ initial, onComplete, previewMode = false }: { initial: OnboardingState; onComplete: () => void; previewMode?: boolean }) {
  const [step, setStep] = useState(1);
  const [state, setState] = useState(initial);
  const [main, setMain] = useState<MainProjection | null>(null);
  const [tunnelId, setTunnelId] = useState("");
  const [runtimeKey, setRuntimeKey] = useState("");
  const [selectedFolder, setSelectedFolder] = useState("");
  const [rememberedProject, setRememberedProject] = useState("");
  const [permission, setPermission] = useState<AccessCode>("edit");
  const [connectorEndpoint, setConnectorEndpoint] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState("");
  const [copiedRows, setCopiedRows] = useState<Record<Screen4CopyKey, boolean>>({ name: false, tunnel: false });
  const copyTimers = useRef<Record<Screen4CopyKey, number | null>>({ name: null, tunnel: null });
  const [preparingProject, setPreparingProject] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const allGreen = state.readiness.localEnvironment && state.readiness.codingService && state.readiness.openaiTunnel;

  useEffect(() => () => {
    for (const timer of Object.values(copyTimers.current)) {
      if (timer !== null) window.clearTimeout(timer);
    }
  }, []);

  useEffect(() => {
    void bridge.read().then((projection) => {
      setMain(projection);
      setPermission(projection.permission);
      const active = projection.projects.find((item) => item.active);
      if (active) setRememberedProject(active.id);
    }).catch(() => setError("无法读取项目设置"));
  }, []);

  useEffect(() => {
    if (step !== 4 && step !== 5 && step !== 6) return;
    let active = true;
    const refresh = async () => {
      try {
        const [next, endpoint] = await Promise.all([
          onboardingApi.read(),
          onboardingApi.readConnectorEndpoint(),
        ]);
        if (!active) return;
        setState(next);
        setConnectorEndpoint(endpoint.endpoint);
      } catch {
        if (active) setError("无法检查本地服务状态");
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [step]);

  const chosenProject = useMemo(() => main?.projects.find((item) => item.id === rememberedProject) ?? null, [main, rememberedProject]);

  const saveConnection = async () => {
    setError(null);
    try {
      await onboardingApi.saveConnection(tunnelId.trim(), runtimeKey);
      setRuntimeKey("");
      setState(await onboardingApi.read());
      setStep(3);
    } catch (value) {
      setError(messageFrom(value, "OpenAI 连接设置未保存"));
    }
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
      await bridge.setAccess(permission);
      let selectedProject = rememberedProject;
      if (selectedFolder) selectedProject = await onboardingApi.rememberProject(selectedFolder);
      if (!selectedProject) throw new Error("请选择项目文件夹");
      setRememberedProject(selectedProject);
      await onboardingApi.startProject(selectedProject);
      let readyState: OnboardingState | null = null;
      for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt += 1) {
        const next = await onboardingApi.read();
        setState(next);
        if (next.readiness.localEnvironment && next.readiness.codingService && next.readiness.openaiTunnel) {
          readyState = next;
          break;
        }
        await new Promise<void>((resolve) => window.setTimeout(resolve, READY_POLL_INTERVAL_MS));
      }
      if (!readyState) throw new Error("本地服务未在预期时间内就绪，请重试");
      setState(readyState);
      setSelectedFolder("");
      setStep(4);
    } catch (value) {
      setError(messageFrom(value, "本地服务启动失败，请重试"));
    } finally {
      setPreparingProject(false);
    }
  };

  const startSelectedProject = async () => {
    if (!rememberedProject) {
      setError("请选择项目文件夹");
      return;
    }
    setError(null);
    try {
      await onboardingApi.startProject(rememberedProject);
      let next = await onboardingApi.read();
      for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt += 1) {
        setState(next);
        if (next.readiness.localEnvironment && next.readiness.codingService && next.readiness.openaiTunnel) break;
        await new Promise<void>((resolve) => window.setTimeout(resolve, READY_POLL_INTERVAL_MS));
        next = await onboardingApi.read();
      }
      setState(next);
      const endpoint = await onboardingApi.readConnectorEndpoint();
      setConnectorEndpoint(endpoint.endpoint);
    } catch (value) {
      setError(messageFrom(value, "本地服务启动失败，请重试"));
    }
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

  const copyEndpoint = async () => {
    if (!connectorEndpoint) return;
    try {
      await navigator.clipboard.writeText(connectorEndpoint);
      setCopyStatus("已复制");
    } catch {
      setCopyStatus("复制失败");
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

  if (step === 1) return (
    <WizardFrame step={1} title="简单设置 即可开始" footer={<button className="primary" onClick={() => setStep(2)}>开始</button>}>
      <p className="onboarding-copy">LocalBridge是链接ChatGPT与本地代码的工具</p>
    </WizardFrame>
  );

  if (step === 2) return (
    <WizardFrame step={2} title="OpenAI 设置" footer={<><button className="secondary" onClick={() => setStep(1)}>返回</button><button className="primary" disabled={!tunnelId.trim() || (!runtimeKey && !state.runtimeKeySaved)} onClick={() => void saveConnection()}>继续</button></>}>
      <div className="onboarding-field"><label htmlFor="tunnel-id">Tunnel ID</label><input id="tunnel-id" value={tunnelId} onChange={(event) => setTunnelId(event.target.value)} placeholder="tunnel_…" autoComplete="off" /></div>
      <div className="onboarding-link-row"><button className="secondary" onClick={() => void onboardingApi.openTunnelSettings().catch(() => setError("无法打开 Tunnel ID 设置"))}>打开 Tunnel ID 设置</button></div>
      <div className="onboarding-field"><label htmlFor="runtime-key-onboarding">Runtime API Key</label><input id="runtime-key-onboarding" type="password" value={runtimeKey} onChange={(event) => setRuntimeKey(event.target.value)} placeholder={state.runtimeKeySaved ? "已安全保存，可保持不变" : "输入 Runtime API Key"} autoComplete="off" /></div>
      <div className="onboarding-link-row"><button className="secondary" onClick={() => void onboardingApi.openApiKeys().catch(() => setError("无法打开 Runtime API Key 设置"))}>打开 Runtime API Key 设置</button></div>
      <p className="onboarding-hint">{KEY_HINT}</p>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 3) return (
    <WizardFrame step={3} title="项目与权限" footer={<><button className="secondary" disabled={preparingProject} onClick={() => setStep(2)}>返回</button><button className="primary" disabled={preparingProject || (!selectedFolder && !rememberedProject)} onClick={() => void saveProjectAndPermission()}>{preparingProject ? "正在启动…" : "继续"}</button></>}>
      {main?.projects.length ? <div className="onboarding-field"><label htmlFor="remembered-project">已保存项目</label><select id="remembered-project" value={rememberedProject} onChange={(event) => { setRememberedProject(event.target.value); setSelectedFolder(""); }}><option value="">选择项目</option>{main.projects.map((item) => <option key={item.id} value={item.id}>{item.path}</option>)}</select></div> : null}
      <div className="onboarding-folder-row"><button className="secondary" onClick={() => void chooseFolder()}>选择项目文件夹</button><span className="onboarding-selected-folder">{selectedFolder || chosenProject?.path || "尚未选择"}</span></div>
      <div className="onboarding-permissions">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} className={`choice onboarding-permission ${permission === mode ? "selected" : ""}`} aria-pressed={permission === mode} onClick={() => setPermission(mode)}><strong>{accessText[mode]}</strong><small>{mode === "edit" ? "读取、搜索和修改项目文件" : mode === "full" ? "允许运行测试、编译和其他本地命令" : "在完整模式基础上允许显式管理员操作"}</small></button>)}</div>
      {permission === "admin" ? <p className="onboarding-hint">管理员模式不会自动弹出系统授权窗口；需要时再由你显式启用。</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 4) return (
    <WizardFrame step={4} title="创建自定义插件" footer={<button className="primary" disabled={!allGreen} onClick={() => { setError(null); setStep(5); }}>继续</button>}>
      <p className="onboarding-copy">在插件设置页面最底端，打开“开发者模式”</p>
      <div className="onboarding-plugin-top-action"><button className="secondary" onClick={() => void onboardingApi.openPluginsSettings().catch(() => setError("无法打开 ChatGPT插件设置"))}>打开 ChatGPT插件设置</button></div>
      <div className="onboarding-plugin-info">
        <div className="onboarding-info-row"><span className="onboarding-info-label">名称</span><span className="onboarding-info-value">Local Bridge</span><button className={`secondary onboarding-copy-action ${copiedRows.name ? "copied" : ""}`} onClick={() => void copyScreen4Value("name", "Local Bridge")}>{copiedRows.name ? "已复制" : "复制"}</button></div>
        <div className="onboarding-info-row"><span className="onboarding-info-label">Tunnel ID</span><span className="onboarding-info-value">{state.tunnelId ?? "—"}</span><button className={`secondary onboarding-copy-action ${copiedRows.tunnel ? "copied" : ""}`} disabled={!state.tunnelId} onClick={() => state.tunnelId && void copyScreen4Value("tunnel", state.tunnelId)}>{copiedRows.tunnel ? "已复制" : "复制"}</button></div>
      </div>
      <div className="onboarding-plugin-management"><button className="secondary" disabled={!allGreen} onClick={() => void onboardingApi.openConnectorSettings().catch(() => setError("无法打开插件管理页"))}>打开插件管理页</button></div>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 5) return (
    <WizardFrame step={5} title="Local Bridge 使用确认" footer={<><button className="secondary" onClick={() => setStep(4)}>返回</button><button className="primary" onClick={() => { setError(null); setStep(6); }}>继续</button></>}>
      {connectorEndpoint ? <div className="onboarding-endpoint"><code>{connectorEndpoint}</code><button className="secondary" onClick={() => void copyEndpoint()}>复制 Local Bridge 地址</button></div> : <p className="onboarding-hint">Local Bridge 地址将在 OpenAI Tunnel 就绪后显示。</p>}
      <div className="onboarding-copy-feedback" aria-live="polite">{copyStatus || "\u00a0"}</div>
      <p className="onboarding-copy">返回 ChatGPT 后即可尝试选择 Local Bridge；LocalBridge 不判断 ChatGPT 是否已连接。</p>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  return (
    <WizardFrame step={6} title="启动检查" footer={<><button className="secondary" onClick={() => setStep(5)}>返回</button>{!allGreen ? <button className="secondary" onClick={() => void startSelectedProject()}>重试</button> : null}<button className="primary" disabled={!allGreen} onClick={() => void finish()}>确定</button></>}>
      <div className="readiness-list">
        <ReadinessCheck label="本地运行环境" ready={state.readiness.localEnvironment} />
        <ReadinessCheck label="编码服务" ready={state.readiness.codingService} />
        <ReadinessCheck label="OpenAI Tunnel" ready={state.readiness.openaiTunnel} />
      </div>
      {allGreen ? <p className="onboarding-success">配置完成，在插件中选择刚刚添加的Local Bridge试试吧</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );
}
