import type { UiError } from "../bridge";
import { uiErrorText } from "../presentation";

export function UiErrorNotice({ error }: { error: UiError }) {
  return <div className="error" role="alert">
    <span>{uiErrorText(error)}</span>
    <details><summary>{error.code}</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(error, null, 2)}</pre></details>
  </div>;
}
