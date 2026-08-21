import type { ReactNode } from "react";

export function WizardFrame({ step, title, children, footer }: { step: number; title: string; children: ReactNode; footer: ReactNode }) {
  return (
    <main className="onboarding-shell">
      <section className="onboarding-page" aria-labelledby="onboarding-title">
        <div className="onboarding-step">{step} / 5</div>
        <h1 id="onboarding-title">{title}</h1>
        <div className="onboarding-body">{children}</div>
        <div className="onboarding-footer">{footer}</div>
      </section>
    </main>
  );
}
