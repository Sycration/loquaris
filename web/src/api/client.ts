// Typed wrapper around the REST API. Every request sends the session cookie
// and the X-Loquaris header the server requires on state-changing requests
// (its CSRF defence).
import type * as T from './types'

export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

async function request<R>(method: string, path: string, body?: unknown): Promise<R> {
  const headers: Record<string, string> = { 'X-Loquaris': '1' }
  let payload: BodyInit | undefined
  if (body instanceof Blob || body instanceof ArrayBuffer) {
    payload = body
    if (body instanceof Blob && body.type) headers['Content-Type'] = body.type
  } else if (body !== undefined) {
    headers['Content-Type'] = 'application/json'
    payload = JSON.stringify(body)
  }
  const response = await fetch(path, { method, headers, body: payload, credentials: 'same-origin' })
  if (!response.ok) {
    let message = response.statusText || `Request failed (${response.status})`
    try {
      const error = (await response.json()) as T.ApiErrorBody
      if (error.error) message = error.error
    } catch {
      // Not JSON; keep the status text.
    }
    throw new ApiError(response.status, message)
  }
  if (response.status === 204) return undefined as R
  const type = response.headers.get('Content-Type') ?? ''
  return (type.includes('application/json') ? response.json() : response.blob()) as Promise<R>
}

const enc = encodeURIComponent

export const api = {
  config: () => request<T.ServerConfig>('GET', '/api/config'),

  auth: {
    me: () => request<T.User>('GET', '/api/auth/me'),
    register: (body: T.RegisterRequest) => request<T.User>('POST', '/api/auth/register', body),
    login: (body: T.LoginRequest) => request<T.User>('POST', '/api/auth/login', body),
    logout: () => request<void>('POST', '/api/auth/logout'),
    updateMe: (body: T.UpdateMeRequest) => request<T.User>('PATCH', '/api/auth/me', body),
    changePassword: (body: T.ChangePasswordRequest) =>
      request<void>('POST', '/api/auth/password', body),
    resetPassword: (body: T.ResetPasswordRequest) => request<T.User>('POST', '/api/auth/reset', body),
    sessions: () => request<T.Session[]>('GET', '/api/auth/sessions'),
    revokeSession: (id: string) => request<void>('DELETE', `/api/auth/sessions/${enc(id)}`),
    /** Signs out every session except the current one. */
    revokeOtherSessions: () => request<void>('DELETE', '/api/auth/sessions'),
  },

  projects: {
    list: () => request<T.ProjectSummary[]>('GET', '/api/projects'),
    create: (body: T.CreateProjectRequest) => request<T.ProjectSummary>('POST', '/api/projects', body),
    /** Body: the bytes of a desktop .loqr file. */
    import: (file: Blob) =>
      request<T.ProjectSummary>('POST', '/api/projects/import', new Blob([file], { type: 'application/octet-stream' })),
    get: (id: string) => request<T.ProjectDetail>('GET', `/api/projects/${enc(id)}`),
    update: (id: string, body: T.UpdateProjectRequest) =>
      request<T.ProjectDetail>('PATCH', `/api/projects/${enc(id)}`, body),
    remove: (id: string) => request<void>('DELETE', `/api/projects/${enc(id)}`),
    duplicate: (id: string) => request<T.ProjectSummary>('POST', `/api/projects/${enc(id)}/duplicate`),

    addMember: (id: string, body: T.AddMemberRequest) =>
      request<T.Member>('POST', `/api/projects/${enc(id)}/members`, body),
    updateMember: (id: string, userId: string, body: T.UpdateMemberRequest) =>
      request<T.Member>('PUT', `/api/projects/${enc(id)}/members/${enc(userId)}`, body),
    /** Removing yourself leaves the project. */
    removeMember: (id: string, userId: string) =>
      request<void>('DELETE', `/api/projects/${enc(id)}/members/${enc(userId)}`),

    createInvite: (id: string, body: T.CreateInviteRequest) =>
      request<T.CreateInviteResponse>('POST', `/api/projects/${enc(id)}/invites`, body),
    revokeInvite: (id: string, inviteId: string) =>
      request<void>('DELETE', `/api/projects/${enc(id)}/invites/${enc(inviteId)}`),

    uploadAudio: (id: string, wav: Blob) =>
      request<T.AudioRef>('POST', `/api/projects/${enc(id)}/audio`, new Blob([wav], { type: 'audio/wav' })),

    versions: (id: string) => request<T.Version[]>('GET', `/api/projects/${enc(id)}/versions`),
    createVersion: (id: string, body: T.CreateVersionRequest) =>
      request<T.Version>('POST', `/api/projects/${enc(id)}/versions`, body),
    restoreVersion: (id: string, versionId: string) =>
      request<void>('POST', `/api/projects/${enc(id)}/versions/${enc(versionId)}/restore`),
  },

  invites: {
    preview: (token: string) => request<T.InvitePreview>('GET', `/api/invites/${enc(token)}`),
    accept: (token: string) => request<T.AcceptInviteResponse>('POST', `/api/invites/${enc(token)}/accept`),
  },

  admin: {
    users: () => request<T.AdminUser[]>('GET', '/api/admin/users'),
    createUser: (body: T.AdminCreateUserRequest) =>
      request<T.ResetLinkResponse>('POST', '/api/admin/users', body),
    updateUser: (id: string, body: T.AdminUpdateUserRequest) =>
      request<T.AdminUser>('PATCH', `/api/admin/users/${enc(id)}`, body),
    resetLink: (id: string) => request<T.ResetLinkResponse>('POST', `/api/admin/users/${enc(id)}/reset-link`),
  },
}

/** URLs fetched directly by the browser (audio elements, downloads). */
export const urls = {
  audio: (blob: string) => `/api/audio/${enc(blob)}`,
  /** `lessons`: lesson IDs; all lessons when omitted. */
  exportPdf: (id: string, lessons?: string[]) =>
    `/api/projects/${enc(id)}/export/pdf${lessons ? `?lessons=${lessons.map(enc).join(',')}` : ''}`,
  exportWav: (id: string, options: { lessons?: string[]; teacher: boolean; student: boolean }) => {
    const query = new URLSearchParams({ teacher: String(options.teacher), student: String(options.student) })
    if (options.lessons) query.set('lessons', options.lessons.join(','))
    return `/api/projects/${enc(id)}/export/wav?${query}`
  },
  exportLoqr: (id: string) => `/api/projects/${enc(id)}/export/loqr`,
  /** WebsocketProvider server URL; the room name is `${projectId}/sync`. */
  syncServer: () => `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/projects`,
}
