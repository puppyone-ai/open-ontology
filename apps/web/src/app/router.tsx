import { Navigate, type RouteObject } from "react-router";
import { guardedAdminRoutes } from "./admin-gate";
import { ADMIN_MODULES } from "./module-registration";
import { AdminLoginPage } from "../features/auth/admin-login-page";
import { BootstrapPage } from "../features/auth/bootstrap-page";
import { ForbiddenPage, NotFoundPage, RouteFailure, UnsupportedPage } from "../features/auth/error-pages";
import { LoginPage } from "../features/auth/login-page";
import { AccountPage } from "../features/user/account-page";
import { ConversationFilesPage } from "../features/user/conversation-files-page";
import { ConversationPage } from "../features/user/conversation-page";
import { KnowledgePage } from "../features/user/knowledge-page";

export const appRoutes: RouteObject[] = [
  {
    errorElement: <RouteFailure />,
    children: [
      { path: "/", element: <Navigate to="/login" replace /> },
      { path: "/login", element: <LoginPage /> },
      { path: "/conversations/:conversationId/files", element: <ConversationFilesPage /> },
      { path: "/conversations/:conversationId", element: <ConversationPage /> },
      { path: "/knowledge", element: <KnowledgePage /> },
      { path: "/account", element: <AccountPage /> },
      { path: "/admin/login", element: <AdminLoginPage /> },
      { path: "/admin/bootstrap", element: <BootstrapPage /> },
      { path: "/forbidden", element: <ForbiddenPage /> },
      { path: "/unsupported", element: <UnsupportedPage /> },
      ...guardedAdminRoutes(ADMIN_MODULES),
      { path: "*", element: <NotFoundPage /> },
    ],
  },
];
