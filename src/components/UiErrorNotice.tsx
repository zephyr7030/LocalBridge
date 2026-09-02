import type { UiError } from "../bridge";

export function UiErrorNotice({ error }: { error: UiError }) {
  return <div className="error" role="alert">
    <span>{error.message}</span>
    <details><summary>{error.code}</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(error, null, 2)}</pre></details>
  </div>;
}
