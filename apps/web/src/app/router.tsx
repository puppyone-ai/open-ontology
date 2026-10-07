import { Navigate, type RouteObject } from "react-router";
import { ADMIN_MODULES, routesFromModules } from "./module-registration";
import { AdminLoginPage } from "../features/auth/admin-login-page";
import { BootstrapPage } from "../features/auth/bootstrap-page";
import { ForbiddenPage, NotFoundPage, RouteFailure, UnsupportedPage } from "../features/auth/error-pages";
import { LoginPage } from "../features/auth/login-page";

export const appRoutes: RouteObject[] = [
  {
    errorElement: <RouteFailure />,
    children: [
      { path: "/", element: <Navigate to="/login" replace /> },
      { path: "/login", element: <LoginPage /> },
      { path: "/admin", element: <Navigate to="/admin/login" replace /> },
      { path: "/admin/login", element: <AdminLoginPage /> },
      { path: "/admin/bootstrap", element: <BootstrapPage /> },
      { path: "/forbidden", element: <ForbiddenPage /> },
      { path: "/unsupported", element: <UnsupportedPage /> },
      ...routesFromModules(ADMIN_MODULES),
      { path: "*", element: <NotFoundPage /> },
    ],
  },
];
