import { useEffect, useState } from "react";
import { bridge } from "./bridge";
import { Onboarding } from "./features/onboarding/Onboarding";
import { onboardingApi, type OnboardingState } from "./features/onboarding/api";
import { Dashboard } from "./features/dashboard/Dashboard";
import { WindowChrome } from "./components/WindowChrome";
import "./styles.css";

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

  const openWelcome = () => {
    void onboardingApi
      .read()
      .then((current) => {
        setOnboarding(current);
        setOnboardingPreview(true);
      })
      .catch(() => setOnboardingError(true));
  };

  const content = onboardingError ? (
    <main className="onboarding-loading">无法读取首次设置状态</main>
  ) : !onboarding ? (
    <main className="onboarding-loading">正在准备 LocalBridge…</main>
  ) : !onboarding.complete ? (
    <Onboarding
      initial={onboarding}
      onComplete={() => setOnboarding({ ...onboarding, complete: true })}
    />
  ) : onboardingPreview ? (
    <Onboarding initial={onboarding} previewMode onComplete={() => setOnboardingPreview(false)} />
  ) : (
    <Dashboard onOpenWelcome={openWelcome} />
  );

  return <WindowChrome>{content}</WindowChrome>;
}
