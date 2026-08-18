import { readFileSync } from "node:fs";
const app = readFileSync("src/App.tsx", "utf8");
if (!app.includes('className="task-row"') || !app.includes("currentActivityText(currentWorkflow, currentCommand)")) throw new Error("ARCH-013 schema42 current activity row missing");
if (!app.includes('className="last-tool-row"') || !app.includes("lastToolText(projection.lastTool)") || !app.includes("formatLastToolAge(projection.lastTool.ageMs)")) throw new Error("ARCH-013 single last-tool row missing");
if (app.includes("lastCommandAgeMs")) throw new Error("ARCH-013 age leaked back onto current row");
console.log("ARCH-013_VERIFY=PASS");
