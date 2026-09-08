import { useEffect, useRef, useState } from "react";
import { bridge } from "../bridge";

export function adminWarningRemainingSeconds(notBeforeUnixMs: number, nowUnixMs: number): number {
  return Math.ceil(Math.max(0, notBeforeUnixMs - nowUnixMs) / 1000);
}

export function adminWarningCanConfirm(notBeforeUnixMs: number, nowUnixMs: number): boolean {
  return nowUnixMs >= notBeforeUnixMs;
}

// 这里不列举灾难。八条递进的后果只会让人练出视而不见的本事，
// 而且它回答不了用户此刻真正要判断的事：这次授权管多久、之后怎么查、
// 以及自己是不是根本不需要它。
const ADMIN_MODE_FACTS = [
  "这次授权在你切回其他模式之前一直有效，不会每条命令再问你一次。",
  "每条以管理员身份执行的命令都会原样记进本地日志，可以在诊断里逐条查看。",
  "LocalBridge 会挡掉明确指向自身配置的命令，但这是一道浅防线，不是保证。",
] as const;
const ADMIN_WARNING_INITIAL_SECONDS = 3;

export function AdminModeWarning({ onCancel, onConfirm }: { onCancel: () => void; onConfirm: () => void }) {
  const challengeId = useRef(crypto.randomUUID());
  const notBeforeUnixMs = useRef<number | null>(null);
  const confirmationHandedOff = useRef(false);
  const onCancelRef = useRef(onCancel);
  const [backendChallengeReady, setBackendChallengeReady] = useState(false);
  const [remainingSeconds, setRemainingSeconds] = useState(ADMIN_WARNING_INITIAL_SECONDS);

  useEffect(() => { onCancelRef.current = onCancel; }, [onCancel]);

  const cancel = async () => {
    if (confirmationHandedOff.current) return;
    try {
      await bridge.cancelAdminConsent(challengeId.current);
    } finally {
      onCancelRef.current();
    }
  };

  useEffect(() => {
    let disposed = false;
    void bridge.beginAdminConsent(challengeId.current).then((challenge) => {
      if (disposed) return;
      notBeforeUnixMs.current = challenge.notBeforeUnixMs;
      setRemainingSeconds(adminWarningRemainingSeconds(challenge.notBeforeUnixMs, Date.now()));
      setBackendChallengeReady(true);
    }).catch(() => {
      if (!disposed) setBackendChallengeReady(false);
    });
    const update = () => {
      if (notBeforeUnixMs.current !== null) {
        setRemainingSeconds(adminWarningRemainingSeconds(notBeforeUnixMs.current, Date.now()));
      }
    };
    const timer = window.setInterval(update, 100);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") void cancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      window.removeEventListener("keydown", onKeyDown);
      if (!confirmationHandedOff.current) {
        void bridge.cancelAdminConsent(challengeId.current).catch(() => undefined);
      }
    };
  }, []);

  const confirm = async () => {
    if (!backendChallengeReady || notBeforeUnixMs.current === null || !adminWarningCanConfirm(notBeforeUnixMs.current, Date.now())) return;
    try {
      await bridge.confirmAdminConsent(challengeId.current);
    } catch {
      setBackendChallengeReady(false);
      return;
    }
    confirmationHandedOff.current = true;
    onConfirm();
  };

  return <div className="dialog-backdrop admin-warning-backdrop" onMouseDown={() => void cancel()}>
    <section className="dialog admin-warning" role="dialog" aria-modal="true" aria-labelledby="admin-warning-title" onMouseDown={(event) => event.stopPropagation()}>
      <h2 id="admin-warning-title">切换到管理员模式</h2>
      <p>ChatGPT 发来的命令将以管理员身份运行：可以改动系统设置、服务和注册表，也可以读写任何目录，包括其他模式下拒绝的位置。</p>
      <ul>{ADMIN_MODE_FACTS.map((item) => <li key={item}>{item}</li>)}</ul>
      <p className="admin-warning-footer">如果只是让它跑测试、编译或改项目里的文件，完整模式就够了。</p>
      <div className="dialog-actions">
        <button className="secondary" onClick={() => void cancel()}>取消</button>
        <button className="primary admin-warning-confirm" disabled={!backendChallengeReady || remainingSeconds > 0} onClick={() => void confirm()}>{remainingSeconds > 0 ? `确认${remainingSeconds}` : "确认"}</button>
      </div>
    </section>
  </div>;
}
