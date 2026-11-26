import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { markStartup } from "./startup";
markStartup("module_ms");
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
