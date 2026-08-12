import { useEffect, useMemo, useState } from "react";
import { WizardFrame } from "../../components/WizardFrame";
import { ReadinessCheck } from "../../components/ReadinessCheck";
import { bridge, type AccessCode, type MainProjection } from "../../bridge";
import { accessText } from "../../presentation";
import { onboardingApi, type OnboardingState } from "./api";
import "./onboarding.css";

const KEY_HINT = "运行密钥仅保存在 Windows 安全凭据中，不会以明文写入配置文件、日志、命令行或浏览器存储。";

export function Onboarding({ initial, onComplete }: { initial: OnboardingState; onComplete: () => void }) {
  const [step, setStep] = useState(1);
  const [state, setState] = useState(initial);
  const [main, setMain] = useState<MainProjection | null>(null);
  const [tunnelId, setTunnelId] = useState("");
  const [runtimeKey, setRuntimeKey] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [rememberedProject, setRememberedProject] = useState("");
  const [permission, setPermission] = useState<AccessCode>("edit");
  const [error, setError] = useState<string | null>(null);
  const allGreen = state.readiness.localEnvironment && state.readiness.codingService && state.readiness.openaiTunnel;

  useEffect(() => {
    void bridge.read().then((projection) => {
      setMain(projection);
      setPermission(projection.permission);
      const active = projection.projects.find((item) => item.active);
      if (active) setRememberedProject(active.id);
    }).catch(() => setError("无法读取项目设置"));
  }, []);

  useEffect(() => {
    if (step !== 5) return;
    let active = true;
    const refresh = async () => {
      try {
        const next = await onboardingApi.read();
        if (active) setState(next);
      } catch {
        if (active) setError("无法检查本地服务状态");
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => { active = false; window.clearInterval(timer); };
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
      setError(typeof value === "string" ? value : "OpenAI 连接设置未保存");
    }
  };

  const saveProjectAndPermission = async () => {
    setError(null);
    try {
      await bridge.setAccess(permission);
      if (projectPath.trim()) await bridge.addProject(projectPath.trim());
      else if (rememberedProject) await bridge.selectProject(rememberedProject);
      else throw new Error("请选择项目目录");
      setStep(4);
    } catch (value) {
      setError(typeof value === "string" ? value : value instanceof Error ? value.message : "项目或权限设置未完成");
    }
  };

  const finish = async () => {
    if (!allGreen) return;
    setError(null);
    try {
      await onboardingApi.complete();
      onComplete();
    } catch (value) {
      setError(typeof value === "string" ? value : "设置尚未完成");
    }
  };

  if (step === 1) return (
    <WizardFrame step={1} title="简单设置 即可开始" footer={<button className="onboarding-primary" onClick={() => setStep(2)}>开始</button>}>
      <p className="onboarding-copy">LocalBridge是链接ChatGPT与本地代码的工具</p>
    </WizardFrame>
  );

  if (step === 2) return (
    <WizardFrame step={2} title="连接 OpenAI" footer={<><button className="onboarding-secondary" onClick={() => setStep(1)}>返回</button><button className="onboarding-primary" disabled={!tunnelId.trim() || (!runtimeKey && !state.runtimeKeySaved)} onClick={() => void saveConnection()}>继续</button></>}>
      <div className="onboarding-field"><label htmlFor="tunnel-id">Tunnel ID</label><input id="tunnel-id" value={tunnelId} onChange={(event) => setTunnelId(event.target.value)} placeholder="tunnel_…" autoComplete="off" /></div>
      <div className="onboarding-field"><label htmlFor="runtime-key-onboarding">运行密钥</label><input id="runtime-key-onboarding" type="password" value={runtimeKey} onChange={(event) => setRuntimeKey(event.target.value)} placeholder={state.runtimeKeySaved ? "已安全保存，可保持不变" : "输入运行密钥"} autoComplete="off" /></div>
      <p className="onboarding-hint">{KEY_HINT}</p>
      <div className="onboarding-link-row"><button className="onboarding-secondary" onClick={() => void onboardingApi.openChatGpt().catch(() => setError("无法打开 ChatGPT"))}>打开 ChatGPT MCP 应用页</button></div>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 3) return (
    <WizardFrame step={3} title="项目与权限" footer={<><button className="onboarding-secondary" onClick={() => setStep(2)}>返回</button><button className="onboarding-primary" disabled={!projectPath.trim() && !rememberedProject} onClick={() => void saveProjectAndPermission()}>继续</button></>}>
      {main?.projects.length ? <div className="onboarding-field"><label htmlFor="remembered-project">已保存项目</label><select id="remembered-project" value={rememberedProject} onChange={(event) => { setRememberedProject(event.target.value); setProjectPath(""); }}><option value="">选择项目</option>{main.projects.map((item) => <option key={item.id} value={item.id}>{item.path}</option>)}</select></div> : null}
      <div className="onboarding-field"><label htmlFor="project-path-onboarding">项目目录</label><input id="project-path-onboarding" value={projectPath} onChange={(event) => { setProjectPath(event.target.value); if (event.target.value) setRememberedProject(""); }} placeholder={chosenProject?.path ?? "输入代码文件夹路径"} /></div>
      <div className="onboarding-permissions">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} className={`onboarding-permission ${permission === mode ? "selected" : ""}`} aria-pressed={permission === mode} onClick={() => setPermission(mode)}><strong>{accessText[mode]}</strong><small>{mode === "edit" ? "读取、搜索和修改项目文件" : mode === "full" ? "允许运行测试、编译和其他本地命令" : "在完整模式基础上允许显式管理员操作"}</small></button>)}</div>
      {permission === "admin" ? <p className="onboarding-hint">管理员模式不会自动弹出系统授权窗口；需要时再由你显式启用。</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  if (step === 4) return (
    <WizardFrame step={4} title="连接 ChatGPT" footer={<><button className="onboarding-secondary" onClick={() => setStep(3)}>返回</button><button className="onboarding-primary" onClick={() => setStep(5)}>继续</button></>}>
      <p className="onboarding-copy">在 ChatGPT 中选择刚刚配置的 LocalBridge 工具。</p>
      <div className="onboarding-link-row"><button className="onboarding-secondary" onClick={() => void onboardingApi.openChatGpt().catch(() => setError("无法打开 ChatGPT"))}>打开 ChatGPT MCP 应用页</button></div>
      <p className="onboarding-hint">页面只会通过系统默认浏览器打开，LocalBridge 不读取 ChatGPT 会话。</p>
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );

  return (
    <WizardFrame step={5} title="正在准备" footer={<button className="onboarding-primary" disabled={!allGreen} onClick={() => void finish()}>确定</button>}>
      <div className="readiness-list">
        <ReadinessCheck label="本地运行环境" ready={state.readiness.localEnvironment} />
        <ReadinessCheck label="编码服务" ready={state.readiness.codingService} />
        <ReadinessCheck label="OpenAI Tunnel" ready={state.readiness.openaiTunnel} />
      </div>
      {allGreen ? <p className="onboarding-success">设置完成，尝试在插件中选择刚刚添加的工具吧！</p> : null}
      {error && <p className="onboarding-error" role="alert">{error}</p>}
    </WizardFrame>
  );
}
