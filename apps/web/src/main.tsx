import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { appRoutes } from "./app/router";
import "./styles/app.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root");
}

const router = createBrowserRouter(appRoutes);

createRoot(root).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
