import { lazy, Suspense } from 'react'
import { BrowserRouter, Route, Routes } from 'react-router'
import { RequireAuth } from './auth/RequireAuth'
import { AdminPage } from './pages/AdminPage'
import { AppLayout, NotFoundPage } from './pages/AppLayout'
import { LoginPage, RegisterPage, ResetPasswordPage } from './pages/AuthPages'
import { InvitePage } from './pages/InvitePage'
import { ProjectsPage } from './pages/ProjectsPage'
import { SettingsPage } from './pages/SettingsPage'
import { ConfirmHost } from './ui/Dialog'
import { PageLoading } from './ui/misc'
import { ToastViewport } from './ui/Toasts'

// The editor (Yjs, collaboration) loads on demand.
const EditorPage = lazy(() => import('./editor/EditorPage').then((module) => ({ default: module.EditorPage })))

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/register" element={<RegisterPage />} />
      <Route path="/reset/:token" element={<ResetPasswordPage />} />
      <Route path="/invite/:token" element={<InvitePage />} />
      <Route element={<RequireAuth />}>
        <Route element={<AppLayout />}>
          <Route index element={<ProjectsPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/admin" element={<AdminPage />} />
        </Route>
        <Route
          path="/p/:projectId"
          element={
            <Suspense fallback={<PageLoading label="Opening project…" />}>
              <EditorPage />
            </Suspense>
          }
        />
      </Route>
      <Route path="*" element={<NotFoundPage />} />
    </Routes>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <AppRoutes />
      <ConfirmHost />
      <ToastViewport />
    </BrowserRouter>
  )
}
