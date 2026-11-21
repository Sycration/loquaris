# Loquaris web: design

Loquaris is being rebuilt as a collaborative web application: several people
can edit the same project at once, as in Google Docs, each signed in with their
own account. This document records the architecture and the decisions behind
it. The desktop app it replaces is in git history (up to `5877237`).

## Goals

- Every feature of the desktop app, in the browser: script editing, recording
  and cropping audio, lesson playback, auto-macronizing, search with lemma
  matching, parsing review, first-use words, WAV and PDF export.
- Real-time collaboration: edits from every participant merge without
  conflicts or locking, appear for others within a fraction of a second, and
  survive a dropped connection. Collaborators see who else is in the project
  and where they are working.
- Accounts, projects shared with other accounts, and per-project roles.
- Desktop `.loqr` projects can be imported, so no work is lost.
- One server binary and one data directory, easy to self-host.
- `collo` remains an independent library with no knowledge of Loquaris.

Non-goals for the first release: comments and suggestions, offline editing
across browser restarts, horizontal scaling across several server processes,
email (password resets go through admins), and single sign-on (the auth layer
is written so OIDC can be added).

## Architecture

```
 Browser                                       Server (one Rust process)
┌────────────────────────────────────┐        ┌───────────────────────────────┐
│ React UI                           │  HTTPS │ axum                          │
│  ├─ Y.Doc  (the project, a CRDT) ◄─┼────────┼─► /api  REST: auth, projects, │
│  ├─ Y.UndoManager (own edits only) │   WS   │     members, audio, export,   │
│  ├─ Awareness (presence, cursors)◄─┼────────┼─► /api/projects/:id/sync      │
│  └─ Web Audio (record, play)       │        │      Yjs sync + awareness     │
│ Web Worker                         │        │  ├─ rooms: yrs Doc per open   │
│  └─ loquaris-core (wasm)           │        │  │   project, in memory       │
│      collo, word index, search,    │        │  ├─ SQLite: users, sessions,  │
│      macronize                     │        │  │   projects, members, doc   │
└────────────────────────────────────┘        │  │   updates and snapshots    │
                                              │  ├─ blob store: audio files   │
                                              │  └─ export: PDF, WAV, .loqr   │
                                              └───────────────────────────────┘
```

### Repository layout

```
collo/                 Latin analyzer library (independent; usable anywhere)
crates/core/           loquaris-core: project model, word index, search,
                       macronize, lemma choices. Pure Rust, no I/O; builds
                       natively and for wasm32.
crates/core-wasm/      wasm-bindgen wrapper around loquaris-core for the
                       browser worker.
crates/export/         loquaris-export: PDF and WAV rendering and the desktop
                       .loqr reader. Native only.
crates/server/         loquaris-server: HTTP API, auth, collaboration rooms,
                       persistence, blob store; serves the built web app.
web/                   The browser app: Vite, React, TypeScript, Yjs.
docs/                  This document.
```

## Key decisions

**Yjs as the collaboration model.** The project lives in a CRDT document. Each
browser holds a full replica and edits it locally, so typing never waits on
the network; replicas exchange updates and converge. Yjs is the most mature
CRDT for text, and `yrs` is its Rust port with a byte-compatible encoding, so
the server can hold, persist, inspect and export documents natively. This is
preferred to operational transformation (needs a central transform server and
hard-to-get-right transforms per operation type) and to locking (not
"Google Docs-like").

**A Rust server.** It reuses collo and the existing PDF/WAV/.loqr code
directly, and `yrs` gives it a real copy of each document for exports and
validation. axum + tokio + sqlx.

**SQLite and a file blob store.** One process holds every open document in
memory, so a networked database buys nothing at the expected scale (a school
or department: tens of concurrent editors). SQLite in WAL mode, accessed
through sqlx, keeps deployment to one binary and one directory, and backup to
copying that directory. Audio is content-addressed files on disk behind a
small `BlobStore` trait that an S3 implementation can satisfy later.

**Latin analysis in the browser.** collo has no dependencies and compiles to
WebAssembly (about 3 MB with Brotli, cached indefinitely; built without collo's rare-word extension lexicon, which Loquaris never used). Running it in a
worker makes search, parsing review and macronize instant and keeps the server
free of per-keystroke work. Every client derives the same index from the same
document, so results agree between collaborators. Lemma choices are stored in
the document, not derived per client.

**Audio outside the CRDT.** Audio is uploaded once, stored immutably under its
SHA-256, and referenced from the document by ID. Crops stay non-destructive:
an entry stores the clip reference and a frame range, exactly as before.
Undoing a recording just restores the previous reference.

**React + TypeScript.** The egui UI cannot be reused: canvas rendering breaks
text input methods, accessibility and browser text features that a script
editor needs. React is the most widely understood choice for a DOM UI of this
size.

## The collaborative document

One `Y.Doc` per project. Order is not stored as array position: concurrent
moves in a `Y.Array` duplicate items, and moving an item (delete + insert)
discards concurrent edits to it. Instead lessons and entries live in maps
keyed by a stable ID and carry a fractional-index `order` key. A move only
changes `order` (and `lesson` for entries), so it is a last-writer-wins field
update that never duplicates or loses text. Ties in `order` break by ID.

```
doc
├─ meta: Y.Map
│   ├─ name: string                project title
│   ├─ l1Name: string              learner's language, e.g. "English"
│   ├─ gapSameMs: number           silence between lines of one speaker (100)
│   ├─ gapDifferentMs: number      silence when the speaker changes (500)
│   ├─ paper: "a4"|"letter"|"legal"|"a3"
│   └─ fontSize: number            PDF body size in points
├─ lessons: Y.Map<lessonId, Y.Map>
│   ├─ name: Y.Text
│   └─ order: string               fractional index
└─ entries: Y.Map<entryId, Y.Map>
    ├─ lesson: lessonId
    ├─ order: string               fractional index within the lesson
    ├─ participant: "teacher"|"student"
    ├─ language: "l1"|"latin"      students always speak Latin
    ├─ text: Y.Text
    ├─ audio: {blob, sampleRate, frames} | null
    ├─ trim: {start, end} | null   frames, end exclusive; null = whole clip
    └─ lemmas: Y.Map<"token#occurrence", {key, extension}>
```

IDs are random 16-character base-62 strings (whoever creates the object
generates it). Deleting a lesson deletes its entries in the same transaction.
Text is edited through `Y.Text`, so concurrent typing in the same line merges
character by character; macronizing applies a minimal diff rather than
replacing the line.

Encoding rules, binding on both the TypeScript and the Rust (yrs) side:

- The three root types are `doc.getMap("meta")`, `doc.getMap("lessons")`,
  `doc.getMap("entries")`. `meta` also holds `schema: 1`.
- Each lesson and entry is a nested `Y.Map`; `name` and `text` are nested
  `Y.Text`; `lemmas` is a nested `Y.Map`. Everything else is a plain value
  (`Any`): strings, numbers, booleans, or JSON objects that are replaced as a
  whole (`audio`, `trim`, each lemma choice).
- `audio` is `{blob: string, sampleRate: number, frames: number,
  channels: number}`; `blob` is the lowercase hex SHA-256 of the WAV file.
  `trim` is `{start: number, end: number}`. A lemma choice is stored under
  the key `` `${token}#${occurrence}` `` as `{key: string, extension:
  boolean}`.
- Numbers may arrive from yrs as integers or floats; readers accept both.
- Order keys are generated with the `fractional-indexing` package in the
  browser and `loquaris_core::order` (a compatible port) on the server, and
  are compared as plain strings (`a < b`, never `localeCompare`), ties broken
  by ID.
- An entry whose `lesson` names a lesson that no longer exists (a concurrent
  delete) is not shown, and is removed by the next client that notices.
- Missing fields take their defaults; unknown fields are preserved.

The lesson collapse state is per-user UI state and is kept in local storage,
not in the document.

The schema lives in `web/src/doc/schema.ts` (reading and writing helpers)
and is mirrored in `crates/server/src/doc.rs` (reading only, for exports and
the `.loqr` import, which builds a document through yrs).

### Undo

Each user has a `Y.UndoManager` that tracks only their own transactions, so
undo reverts your edits and never someone else's, as in Google Docs.
Consecutive keystrokes are grouped (500 ms). Structural actions (move, delete,
record, crop, macronize, parsing review) are each one undo step.

### Presence

The Yjs awareness protocol carries each participant's name, colour, the entry
they are focused on, their text selection (as Yjs relative positions, so it
stays correct while others type), and whether they are recording. The editor
shows avatars in the header, a coloured outline and name tag on entries others
are working in, remote carets and selections inside text, and a "recording"
badge. Starting to record over an entry someone else is recording asks first.

## Server

### Sync protocol

`GET /api/projects/:id/sync` upgrades to a WebSocket speaking the standard
y-websocket protocol (sync step 1/2, update, awareness), so the browser uses
stock `y-protocols`. The upgrade checks the session cookie, project
membership, and the `Origin` header (cross-site WebSocket hijacking).
Connections with the viewer role receive updates but their updates are
dropped and the connection is closed if they persist.

Each open project is a room: a yrs `Doc` plus its awareness and connected
peers. Rooms load on first connection and unload a minute after the last
peer leaves.

### Persistence

Every applied update is appended to `doc_updates` before it is broadcast. A
room compacts the log into `doc_snapshots` (full state) every 500 updates and
on unload. Loading reads the latest snapshot plus later updates. A crash loses
nothing that a client was told was saved.

Version history: the server keeps an automatic snapshot per hour of activity
and on request ("Name this version"); restoring computes the old content and
applies it as a new change, so collaborators simply see the document change.

### Database

```
users        id, email (unique, case-folded), display_name, password_hash
             (argon2id), is_admin, settings (JSON: keybindings, preferences),
             created_at
sessions     token_hash (SHA-256 of a 256-bit random token), user_id,
             created_at, last_seen_at, expires_at, user_agent
projects     id, name (mirrors meta.name for listings), owner_id,
             created_at, updated_at, deleted_at
members      project_id, user_id, role (owner | editor | viewer), added_at
invites      token_hash, project_id, role, created_by, expires_at,
             max_uses, uses
doc_updates  project_id, seq, data, user_id, created_at
doc_snapshots project_id, seq (covers updates ≤ seq), data, created_at
versions     id, project_id, label, data (encoded state), created_by,
             created_at
blobs        sha256, size, mime, sample_rate, frames, created_by,
             created_at
project_blobs project_id, sha256        which projects may read a blob
```

### Accounts and security

- Email and password; passwords hashed with argon2id. The first account
  created becomes an admin. Registration is `open`, `invite` (only with a
  project invite link) or `closed` (admins create accounts), set by
  `LOQ_REGISTRATION`.
- Sessions: an opaque random token in an `HttpOnly; Secure; SameSite=Lax`
  cookie, stored hashed, 30-day sliding expiry; sign-out and "sign out
  everywhere" delete rows.
- CSRF: state-changing requests must be JSON (or a blob upload) and carry
  `X-Loquaris: 1`, which a cross-site form cannot send; `SameSite=Lax` covers
  the rest. WebSocket upgrades check `Origin`.
- Login and registration are rate-limited per IP and per account.
- Password reset: admins generate one-time reset links (7 days) from the
  admin page or the `loquaris-server reset-link <email>` command. Emailed
  links (SMTP) are planned but not in the first release.
- Every project route checks membership and role in one extractor; blob
  reads check that the blob belongs to a project the user can read.

### Roles

| Action                                   | viewer | editor | owner |
|------------------------------------------|:------:|:------:|:-----:|
| Open, play, search, export               |   ✓    |   ✓    |   ✓   |
| Edit script, record, crop, settings      |        |   ✓    |   ✓   |
| Name and restore versions                |        |   ✓    |   ✓   |
| Invite, change roles, remove members     |        |        |   ✓   |
| Rename, transfer, delete project         |        |        |   ✓   |

### REST API

All JSON; errors are `{ "error": "message" }` with a fitting status.

```
POST   /api/auth/register         {email, displayName, password, invite?}
POST   /api/auth/login            {email, password}
POST   /api/auth/logout
GET    /api/auth/me               → user + settings
PATCH  /api/auth/me               {displayName?, settings?}
POST   /api/auth/password         {current, new}
POST   /api/auth/reset            {token, password}
GET    /api/auth/sessions | DELETE /api/auth/sessions[/:id]

GET    /api/projects              → projects I belong to, with role
POST   /api/projects              {name, l1Name} → creates doc with one lesson
POST   /api/projects/import       .loqr body → new project
GET    /api/projects/:id          → metadata, my role, members
PATCH  /api/projects/:id          {name?} (owner)
DELETE /api/projects/:id          (owner; soft delete)
POST   /api/projects/:id/duplicate
GET    /api/projects/:id/members  | PUT/DELETE /api/projects/:id/members/:uid
POST   /api/projects/:id/invites  {role, expiresInDays?, maxUses?} → link
DELETE /api/projects/:id/invites/:id
POST   /api/invites/:token/accept

POST   /api/projects/:id/audio    WAV body → {blob, sampleRate, frames}
GET    /api/audio/:sha256         WAV (immutable cache headers)

GET    /api/projects/:id/export/pdf?lessons=…        PDF
GET    /api/projects/:id/export/wav?lessons=…        WAV, or ZIP for several
GET    /api/projects/:id/export/loqr                 desktop archive
GET    /api/projects/:id/versions | POST (name) | POST /:vid/restore

GET    /api/admin/users | POST | PATCH /:id | POST /:id/reset-link
GET    /api/projects/:id/sync     WebSocket (Yjs)
```

Exports render from the server's copy of the document, so they are always
current and identical for every collaborator.

## Browser app

### Screens

- **Sign in / register / reset password.**
- **Projects:** my projects and shared ones, with role, collaborators and last
  edit; new project; import `.loqr`; accept an invite link.
- **Editor:** the desktop layout carried over: a lesson list with the script
  of each lesson, entry cards with participant, language, text, audio state
  and first-use words; a side panel for search, uses per lesson, and new
  words per lesson; the audio editor; the lesson player; project details;
  export dialog; sharing dialog; version history; presence avatars and
  connection status ("Saved", "Saving…", "Offline: changes kept until
  reconnect").
- **Settings:** profile, password, sessions, keybindings, audio input device.
- **Admin:** users, registration links, password reset links.

### Audio

Recording uses `getUserMedia` (echo cancellation, noise suppression and auto
gain off) and an `AudioWorklet` that captures PCM at the device rate and
streams blocks to the main thread for the live waveform. On stop the take is
processed as on the desktop (10 Hz high-pass per channel to remove DC, then
250 ms linear fades in and out), encoded as 16-bit PCM WAV in the browser,
and uploaded; the entry is updated only after the upload succeeds. Takes are
capped at 10 minutes. Imported WAV files (up to 100 MiB) are uploaded
unchanged; the server fully decodes every upload to validate it.

Waveforms are computed from the decoded clip (min/max per 1/400 s, cached
per blob). Playback, crop preview and lesson playback with participant gaps
use Web Audio, with the 10 ms ramps at crop edges. The input device is chosen
in the recorder and remembered per browser.

### Analysis worker

`loquaris-core` is compiled to wasm (`crates/core-wasm`, built with
wasm-bindgen `--target web` into `web/src/wasm/pkg/`) and runs in a dedicated
worker. The main thread observes the Y.Doc and forwards changes; the worker
keeps the word index up to date and answers queries. Roughly:

```
setProject({l1Name, lessons: [{id, entries: [{id, participant, language,
            text, lemmas: [{token, occurrence, lemmaKey, extension}]}]}]})
setStructure(lessons: [{id, entryIds}])      after moves, inserts, deletes
setEntry(entry) / removeEntry(id)            after content edits
search(query, {exactText, unmarkedShort, lemma?}) →
       {mode, lemma?, hits: [{entryId, start, end}], perLesson:
        [{lessonId, teacher, student}], analysis: [...word readings]}
firstUse() → {entryId: [headword]}           yellow labels on cards
newInLesson(lessonId) → [{key, extension, headword, translation}]
review(entryId) → words with readings, context choice, preselection,
                  and the direct result when unambiguous
applyReview(entryId, choices) → {text, lemmas}
```

Offsets crossing the boundary are UTF-16 code units, as JavaScript strings
use; the wrapper converts from Rust byte offsets. Every call carries the
document revision it was computed for, and stale answers are dropped. The
worker loads after the editor first renders; features that need it show a
loading state until then.

### Spaced repetition flags

The editor flags Latin words that a learner working through the course
would be due to review, but that the script doesn't bring back in time. We
can't know what a learner remembers, so this simulates a diligent learner
who hears each lesson once, one lesson per day, and treats every use of a
word as a review. It is a writing aid, not a prediction. It is on by
default and switched per project (`meta.spacedRepetition`, default `true`,
in Project details), since it describes how the course is meant to be
studied.

**Timeline.** Lesson *k* (from 0) starts at *k* days. Within a lesson, time
advances entry by entry as in lesson playback: each entry lasts its cropped
clip's duration, or 10 seconds without a recording, and the project's
same/different-participant gap separates consecutive entries. An occurrence
of a word happens at the start of its entry.

**Words.** Only Latin entries count, teacher and student lines alike. Each
word counts as its lemma, chosen exactly as for first-use labels: the lemma
chosen in parsing review, else the most frequent lemma among readings that
fit the written vowel marks. Unrecognised words are ignored.

**Schedule** (Anki's defaults, which are familiar and well calibrated). A
lemma has a current interval and is due one interval after it was last
heard. The intervals are two learning steps, 1 minute and 10 minutes, then
1 day on graduating, then ×2.5 for each review after that.

- First occurrence: the interval is 1 minute.
- A later occurrence, at least half the interval after the previous one,
  is a review: it moves to the next interval.
- An earlier occurrence is a reminder: the clock restarts but the interval
  stays.
- An occurrence after the deadline (below) still counts as a review,
  except when more than twice the interval has passed: then the word is
  taken to be forgotten, and it starts again at 1 minute.

**Deadlines.** A due time inside the lesson where the word was last heard is
a deadline as is. A due time beyond that lesson's end is relaxed to the end
of the first lesson that ends at or after it. Day-scale intervals then mean
"bring it back sometime in that lesson", as Anki shows due cards in the next
session, rather than tying a review to a minute of a later lesson. Without
this, nearly every word would be flagged overnight.

**Flags.** When a deadline passes with no occurrence, the lemma is flagged
once, and not again until it is heard. An in-lesson deadline flags the
first entry at or after the due time. A relaxed deadline flags the lesson as
a whole. Deadlines after the end of the script are not flagged. Each flag
carries the lemma (headword, translation), where it was last heard, and how
long ago.

**Display.**
- In-lesson flags: a small orange marker on the card ("⚑ rosa"); the
  tooltip says when it was last heard ("Due for review — last heard 2:10
  earlier, entry 3").
- Lesson flags: in the lesson's footer bar ("⚑ Not reviewed in this lesson:
  rosa, puella").
- Side panel: a "Due for review" section for the active lesson, listing both
  kinds. Clicking a flag reveals where the word was last heard.

The computation is in `loquaris-core` (`spaced.rs`), over the word index's
resolved lemmas, so it updates with every edit. The worker also receives each
entry's duration (absent means no recording) and the project gaps.

## Changes from the desktop app

- No files, Save, Save as or recent projects: the project list replaces them
  and every change is saved continuously. Cmd+S shows "All changes saved".
  Cmd+O opens the project list.
- Lessons can be deleted (with confirmation, undoable), reordered and
  duplicated, which the desktop could not do. A shared project needs them.
- Lesson collapse state is per user, not saved in the project.
- Undo groups typing (500 ms) instead of one step per keystroke, and history
  is linear (Yjs `UndoManager`), not branching.
- The recorder and other windows become dialogs; there is one toast stack.
- A recording applies to its entry even if collaborators edited the entry's
  text meanwhile. If the entry was deleted, the take is discarded with a
  toast. If someone else replaced the clip during the take, the user is
  asked which to keep.
- Export downloads files: one PDF, a single WAV, or a ZIP of one WAV per
  lesson. The remembered export folder and WAV folder name are gone.
- Audio device selection is limited to what browsers expose (input devices,
  no audio-system choice). Diagnostics show the context sample rate,
  channels, peak level and the silence/no-data warnings.
- Shortcuts browsers do not let pages override (Cmd+N, Cmd+T, Cmd+W, Cmd+Tab
  and the like) are rejected in the keybinding editor.

### Keybindings

The desktop app's actions and default shortcuts carry over, adjusted where a
browser reserves a combination. Bindings are per user, stored in the account's
settings, so they follow the user between browsers.

## Testing

- `loquaris-core`: the desktop test suite's model, index, search and
  macronize cases, ported to ID-keyed entries.
- `loquaris-export`: PDF/WAV output on fixtures; reading every `.loqr`
  version.
- Server: integration tests that drive the HTTP API and two WebSocket
  clients editing concurrently, and check convergence, persistence across
  restart, and role enforcement.
- Web: Vitest for document helpers, the text binding and ordering; Playwright
  for end-to-end flows, including two browser contexts editing one project.

## Deployment

```
LOQ_BIND=0.0.0.0:8080
LOQ_DATA_DIR=/var/lib/loquaris          SQLite database and blobs
LOQ_PUBLIC_URL=https://loquaris.example  for links and Origin checks
LOQ_REGISTRATION=open|invite|closed
```

`cargo build --release -p loquaris-server` after `npm run build` in `web/`
produces one binary with the app embedded. A Dockerfile builds both. Run it
behind a TLS-terminating reverse proxy that passes WebSocket upgrades.
