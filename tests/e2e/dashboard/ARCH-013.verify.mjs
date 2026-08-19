import { readFileSync } from "node:fs";
const app = readFileSync("src/App.tsx", "utf8");
if (!app.includes('className="task-row"') || !app.includes("currentActivityText(currentActivity)")) throw new Error("ARCH-013 schema42 current activity row missing");
if (!app.includes('className="last-tool-row"') || !app.includes("lastActivityAction(lastActivity)") || !app.includes("lastActivityOutcome(lastActivity)") || !app.includes("lastActivity.completedAtMs")) throw new Error("ARCH-013 single backend last-activity row missing");
if (app.includes("lastCommandAgeMs")) throw new Error("ARCH-013 age leaked back onto current row");
console.log("ARCH-013_VERIFY=PASS");
