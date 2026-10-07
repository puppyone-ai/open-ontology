import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Foundation } from "./styles/foundation";
import "./styles/app.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root");
}

createRoot(root).render(
  <StrictMode>
    <Foundation />
  </StrictMode>,
);
