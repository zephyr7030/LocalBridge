import { APP_NAME, bootstrapMessage } from "./appModel";

export function App() {
  return (
    <main>
      <h1>{APP_NAME}</h1>
      <p>{bootstrapMessage()}</p>
    </main>
  );
}
