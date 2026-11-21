// JSON shapes of the REST API (docs/DESIGN.md, "REST API"). The server's
// serde types must serialize to exactly these. Timestamps are ISO 8601
// strings; IDs are opaque strings.

export type Role = 'owner' | 'editor' | 'viewer'

export type Registration = 'open' | 'invite' | 'closed'

/** GET /api/config (public) */
export interface ServerConfig {
  registration: Registration
  version: string
}

/** Free-form per-user preferences, stored server-side. */
export interface UserSettings {
  /** Action id → shortcut string such as "Mod+Shift+ArrowUp", or null to
   * unbind. Missing actions use their defaults. */
  keybindings?: Record<string, string | null>
  [key: string]: unknown
}

/** GET /api/auth/me */
export interface User {
  id: string
  email: string
  displayName: string
  isAdmin: boolean
  settings: UserSettings
}

export interface RegisterRequest {
  email: string
  displayName: string
  password: string
  /** Invite token, required when registration is "invite". */
  invite?: string
}

export interface LoginRequest {
  email: string
  password: string
}

export interface UpdateMeRequest {
  displayName?: string
  settings?: UserSettings
}

export interface ChangePasswordRequest {
  current: string
  new: string
}

export interface ResetPasswordRequest {
  token: string
  password: string
}

/** GET /api/auth/sessions */
export interface Session {
  id: string
  createdAt: string
  lastSeenAt: string
  userAgent: string | null
  current: boolean
}

export interface UserRef {
  id: string
  displayName: string
}

/** GET /api/projects */
export interface ProjectSummary {
  id: string
  name: string
  role: Role
  owner: UserRef
  memberCount: number
  createdAt: string
  updatedAt: string
}

export interface Member {
  userId: string
  email: string
  displayName: string
  role: Role
  addedAt: string
}

export interface Invite {
  id: string
  role: Exclude<Role, 'owner'>
  expiresAt: string | null
  maxUses: number | null
  uses: number
  createdAt: string
  createdBy: UserRef
}

/** GET /api/projects/:id. `invites` is present for owners only. */
export interface ProjectDetail {
  id: string
  name: string
  role: Role
  owner: UserRef
  members: Member[]
  invites?: Invite[]
  createdAt: string
  updatedAt: string
}

/** POST /api/projects */
export interface CreateProjectRequest {
  name: string
  l1Name: string
}

/** PATCH /api/projects/:id */
export interface UpdateProjectRequest {
  name?: string
}

/** POST /api/projects/:id/members. Adds an existing account by email. */
export interface AddMemberRequest {
  email: string
  role: Exclude<Role, 'owner'>
}

/** PUT /api/projects/:id/members/:userId. Setting "owner" transfers
 * ownership; the previous owner becomes an editor. */
export interface UpdateMemberRequest {
  role: Role
}

/** POST /api/projects/:id/invites */
export interface CreateInviteRequest {
  role: Exclude<Role, 'owner'>
  expiresInDays?: number | null
  maxUses?: number | null
}

/** The token appears only in this response; the server stores its hash. */
export interface CreateInviteResponse {
  invite: Invite
  /** Absolute URL of the form {publicUrl}/invite/{token}. */
  url: string
}

/** GET /api/invites/:token (public, so the page can show what it is) */
export interface InvitePreview {
  projectName: string
  role: Exclude<Role, 'owner'>
  invitedBy: UserRef
  /** False if expired, used up or revoked. */
  valid: boolean
}

/** POST /api/invites/:token/accept → the project joined. */
export interface AcceptInviteResponse {
  projectId: string
}

/** POST /api/projects/:id/audio (body: WAV bytes, Content-Type audio/wav).
 * The same shape is stored as an entry's `audio` in the document. */
export interface AudioRef {
  blob: string
  sampleRate: number
  frames: number
  channels: number
}

/** GET /api/projects/:id/versions */
export interface Version {
  id: string
  label: string | null
  auto: boolean
  createdAt: string
  createdBy: UserRef | null
}

export interface CreateVersionRequest {
  label: string
}

/** GET /api/admin/users */
export interface AdminUser {
  id: string
  email: string
  displayName: string
  isAdmin: boolean
  disabled: boolean
  createdAt: string
  projectCount: number
}

/** POST /api/admin/users */
export interface AdminCreateUserRequest {
  email: string
  displayName: string
  isAdmin?: boolean
}

/** PATCH /api/admin/users/:id */
export interface AdminUpdateUserRequest {
  displayName?: string
  isAdmin?: boolean
  disabled?: boolean
}

/** POST /api/admin/users (and /api/admin/users/:id/reset-link): a one-time
 * link to set a password, valid for 7 days. */
export interface ResetLinkResponse {
  url: string
  expiresAt: string
}

/** Every error response. */
export interface ApiErrorBody {
  error: string
}
