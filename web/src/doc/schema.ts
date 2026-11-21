// The project document schema (docs/DESIGN.md, "The collaborative document").
// Reading and writing helpers for the Y.Doc; crates/server/src/doc.rs mirrors
// the reading side. Rules worth repeating here:
//
// - Roots: doc.getMap("meta"), doc.getMap("lessons"), doc.getMap("entries").
// - Lessons and entries are nested Y.Maps keyed by random ID, ordered by a
//   fractional-index `order` string (plain string compare, ties by ID).
// - `name` / `text` are nested Y.Text, `lemmas` a nested Y.Map; everything
//   else is a plain value replaced as a whole.
// - Numbers may arrive from yrs as integers, floats or bigints.
// - Missing fields take defaults; unknown fields are preserved (we never
//   rewrite a whole map, only the keys we change).
import * as Y from 'yjs'
import type { AudioRef } from '../api/types'
import { randomId } from './ids'

export type { AudioRef }
export const SCHEMA_VERSION = 1

export type Participant = 'teacher' | 'student'
export type Language = 'l1' | 'latin'
export type Paper = 'a4' | 'letter' | 'legal' | 'a3'

export interface Trim {
  start: number
  end: number
}

export interface LemmaChoice {
  key: string
  extension: boolean
}

export interface Meta {
  name: string
  l1Name: string
  gapSameMs: number
  gapDifferentMs: number
  paper: Paper
  fontSize: number
  /** Spaced repetition flags (DESIGN.md); on unless explicitly false. */
  spacedRepetition: boolean
}

export const DEFAULT_META: Meta = {
  name: '',
  l1Name: 'English',
  gapSameMs: 100,
  gapDifferentMs: 500,
  paper: 'a4',
  fontSize: 15,
  spacedRepetition: true,
}

/** Gaps are limited to 0–60 s (FEATURES §1, §12). */
export const MAX_GAP_MS = 60_000

/** Transaction origin of this user's edits; the only origin the undo manager
 * tracks. Remote updates come from the provider; schema repairs and orphan
 * cleanup use their own origins so they are never undone. */
export const LOCAL_ORIGIN = 'loquaris:local'
export const SYSTEM_ORIGIN = 'loquaris:system'

export type YLesson = Y.Map<unknown>
export type YEntry = Y.Map<unknown>

export interface Roots {
  meta: Y.Map<unknown>
  lessons: Y.Map<YLesson>
  entries: Y.Map<YEntry>
}

export function roots(doc: Y.Doc): Roots {
  return {
    meta: doc.getMap('meta'),
    lessons: doc.getMap<YLesson>('lessons'),
    entries: doc.getMap<YEntry>('entries'),
  }
}

// ---- plain value readers ---------------------------------------------------

export function readNumber(value: unknown, fallback: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'bigint') return Number(value)
  return fallback
}

export function readString(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value : fallback
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !(value instanceof Y.AbstractType)
}

export function readAudio(value: unknown): AudioRef | null {
  if (!isRecord(value) || typeof value.blob !== 'string') return null
  return {
    blob: value.blob,
    sampleRate: readNumber(value.sampleRate, 48_000),
    frames: readNumber(value.frames, 0),
    channels: readNumber(value.channels, 1),
  }
}

export function readTrim(value: unknown): Trim | null {
  if (!isRecord(value)) return null
  const start = readNumber(value.start, NaN)
  const end = readNumber(value.end, NaN)
  return Number.isFinite(start) && Number.isFinite(end) ? { start, end } : null
}

export function readLemmaChoice(value: unknown): LemmaChoice | null {
  if (!isRecord(value) || typeof value.key !== 'string') return null
  return { key: value.key, extension: value.extension === true }
}

export function readMeta(meta: Y.Map<unknown>): Meta {
  const paper = meta.get('paper')
  return {
    name: readString(meta.get('name'), DEFAULT_META.name),
    l1Name: readString(meta.get('l1Name'), DEFAULT_META.l1Name),
    gapSameMs: readNumber(meta.get('gapSameMs'), DEFAULT_META.gapSameMs),
    gapDifferentMs: readNumber(meta.get('gapDifferentMs'), DEFAULT_META.gapDifferentMs),
    paper: paper === 'letter' || paper === 'legal' || paper === 'a3' || paper === 'a4' ? paper : DEFAULT_META.paper,
    fontSize: readNumber(meta.get('fontSize'), DEFAULT_META.fontSize),
    spacedRepetition: meta.get('spacedRepetition') !== false,
  }
}

// ---- lessons ---------------------------------------------------------------

export function lessonName(lesson: YLesson): Y.Text | null {
  const name = lesson.get('name')
  return name instanceof Y.Text ? name : null
}

export function lessonNameString(lesson: YLesson): string {
  const name = lesson.get('name')
  if (name instanceof Y.Text) return name.toString()
  return typeof name === 'string' ? name : ''
}

export function lessonOrder(lesson: YLesson): string {
  return readString(lesson.get('order'), '')
}

export function createLesson(order: string, name = ''): YLesson {
  const lesson = new Y.Map<unknown>()
  lesson.set('name', new Y.Text(name))
  lesson.set('order', order)
  return lesson
}

// ---- entries ---------------------------------------------------------------

export function entryLesson(entry: YEntry): string {
  return readString(entry.get('lesson'), '')
}

export function entryOrder(entry: YEntry): string {
  return readString(entry.get('order'), '')
}

export function entryParticipant(entry: YEntry): Participant {
  return entry.get('participant') === 'student' ? 'student' : 'teacher'
}

/** Students always speak Latin, whatever is stored. */
export function entryLanguage(entry: YEntry): Language {
  if (entryParticipant(entry) === 'student') return 'latin'
  return entry.get('language') === 'latin' ? 'latin' : 'l1'
}

export function entryText(entry: YEntry): Y.Text | null {
  const text = entry.get('text')
  return text instanceof Y.Text ? text : null
}

export function entryTextString(entry: YEntry): string {
  const text = entry.get('text')
  if (text instanceof Y.Text) return text.toString()
  return typeof text === 'string' ? text : ''
}

export function entryAudio(entry: YEntry): AudioRef | null {
  return readAudio(entry.get('audio'))
}

export function entryTrim(entry: YEntry): Trim | null {
  return readTrim(entry.get('trim'))
}

export function entryLemmas(entry: YEntry): Y.Map<unknown> | null {
  const lemmas = entry.get('lemmas')
  return lemmas instanceof Y.Map ? lemmas : null
}

/** Lemma choices as plain data, keyed `${token}#${occurrence}`. */
export function entryLemmaChoices(entry: YEntry): Record<string, LemmaChoice> {
  const result: Record<string, LemmaChoice> = {}
  entryLemmas(entry)?.forEach((value, key) => {
    const choice = readLemmaChoice(value)
    if (choice) result[key] = choice
  })
  return result
}

export function lemmaKey(token: string, occurrence: number): string {
  return `${token}#${occurrence}`
}

export interface NewEntry {
  lesson: string
  order: string
  participant: Participant
  language: Language
  text?: string
  audio?: AudioRef | null
  trim?: Trim | null
  lemmas?: Record<string, LemmaChoice>
}

export function createEntry(fields: NewEntry): YEntry {
  const entry = new Y.Map<unknown>()
  entry.set('lesson', fields.lesson)
  entry.set('order', fields.order)
  entry.set('participant', fields.participant)
  entry.set('language', fields.participant === 'student' ? 'latin' : fields.language)
  entry.set('text', new Y.Text(fields.text ?? ''))
  entry.set('audio', fields.audio ?? null)
  entry.set('trim', fields.trim ?? null)
  const lemmas = new Y.Map<unknown>()
  for (const [key, choice] of Object.entries(fields.lemmas ?? {})) lemmas.set(key, { ...choice })
  entry.set('lemmas', lemmas)
  return entry
}

/** Make sure an entry or lesson has its Y.Text, converting a legacy plain
 * string if present. Runs as a system change so it is never undone. */
export function ensureText(doc: Y.Doc, map: Y.Map<unknown>, key: 'text' | 'name'): Y.Text {
  const current = map.get(key)
  if (current instanceof Y.Text) return current
  let created: Y.Text | null = null
  doc.transact(() => {
    created = new Y.Text(typeof current === 'string' ? current : '')
    map.set(key, created)
  }, SYSTEM_ORIGIN)
  return created!
}

export { randomId }
