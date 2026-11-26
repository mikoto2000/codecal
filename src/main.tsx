import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { markStartup } from "./startup";
markStartup("module_ms");
// Loading the full app stylesheet must not block the dependency-free shell.
void import("./styles.css").then(() => {
  markStartup("styles_ms");
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
