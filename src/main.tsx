import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { toUserMessage } from "./lib/errors";
import { toast } from "./store/toasts";
import "./styles.css";

const container = document.getElementById("root");

if (!container) {
  throw new Error("Root element #root is missing from index.html");
}

// Anything that escapes the stores' own error handling (a fire-and-forget
// listener, a third-party callback) would otherwise only appear in devtools.
window.addEventListener("unhandledrejection", (event) => {
  console.error("Unhandled promise rejection", event.reason);
  toast.error("Something went wrong", toUserMessage(event.reason));
});

ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
