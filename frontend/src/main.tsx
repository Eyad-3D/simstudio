import React from "react";
import ReactDOM from "react-dom/client";
import "dockview/dist/styles/dockview.css";
import "@xyflow/react/dist/style.css";
import "./index.css";
import App from "./App";
import { useProjectStore } from "./store/projectStore";

if (import.meta.env.DEV) {
  // handy for debugging / E2E tests
  (window as unknown as Record<string, unknown>).__lightsim = useProjectStore;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
