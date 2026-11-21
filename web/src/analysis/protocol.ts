// Messages between the AnalysisClient (main thread) and the analysis worker,
// and the shapes of the analysis results. All text offsets are UTF-16 code
// units, as JavaScript strings use.

export type Participant = 'teacher' | 'student'
export type Language = 'l1' | 'latin'

/** The lemma chosen in parsing review for one occurrence of a word. */
export interface LemmaChoice {
  /** collo-normalized, lowercased spelling. */
  token: string
  /** Index among the entry's words spelled alike, from 0. */
  occurrence: number
  lemmaKey: string
  extension: boolean
}

export interface EntryContent {
  id: string
  participant: Participant
  language: Language
  text: string
  lemmas: LemmaChoice[]
  /** Seconds of the cropped clip; absent without a recording. */
  duration?: number
}

/** Silence between consecutive entries, as in lesson playback. */
export interface Gaps {
  sameParticipantMs: number
  differentParticipantMs: number
}

export type ReviewKind =
  | { type: 'inLesson'; lessonId: string; entryId: string }
  | { type: 'lesson'; lessonId: string }

/** A word due for review that the script does not bring back in time. */
export interface ReviewFlag {
  lemma: Lemma
  kind: ReviewKind
  lastHeardEntryId: string
  lastHeardLessonId: string
  /** Seconds from the last occurrence to the flagged moment. */
  elapsedSeconds: number
  intervalSeconds: number
}

export interface ProjectContent {
  l1Name: string
  lessons: { id: string; entries: EntryContent[] }[]
}

/** A lesson and its entry IDs, in script order. */
export interface LessonOutline {
  id: string
  entryIds: string[]
}

/** Identifies a collo lemma. */
export interface LemmaRef {
  key: string
  extension: boolean
}

export interface Lemma extends LemmaRef {
  /** The headword as a reader expects it, with natural macrons only. */
  headword: string
  /** Meaning in the project's L1 language, if collo has it. */
  translation: string | null
}

export interface SearchOptions {
  exactText?: boolean
  unmarkedShort?: boolean
  /** Search every form of this lemma. Takes precedence over exactText; the
   * UI turns exactText off when a lemma is picked and drops the lemma when
   * the query is edited. */
  lemma?: LemmaRef
}

export interface Hit {
  entryId: string
  start: number
  end: number
}

export interface LessonCount {
  lessonId: string
  teacher: number
  student: number
}

export interface QueryReading {
  /** Reader form of the reading. */
  form: string
  /** In French or Spanish when the L1 is, else English. */
  morphology: string | null
  /** collo's choice in the context of the query (★). */
  contextSelected: boolean
}

export interface QueryLemma extends Lemma {
  readings: QueryReading[]
}

export interface QueryWord {
  text: string
  /** Readings excluded by vowel length. */
  ruledOut: number
  /** Empty: "Not recognised as Latin." */
  lemmas: QueryLemma[]
}

export interface SearchResult {
  mode: 'text' | 'lemma'
  /** The lemma searched, in lemma mode. */
  lemma: Lemma | null
  /** In script order. */
  hits: Hit[]
  /** Every lesson, in script order. */
  perLesson: LessonCount[]
  lessonsWithHits: number
  analysis: QueryWord[]
}

/** Entry ID → reader-form headwords first used in that entry. */
export type FirstUses = Record<string, string[]>

export interface ReviewReading {
  /** The word as this reading spells it; null when it can't be applied
   * safely (disable it in the UI). */
  spelling: string | null
  lemma: Lemma
  morphology: string | null
}

export interface ReviewWord {
  text: string
  start: number
  end: number
  /** In collo's order; choices refer to these indices. Empty: "No dictionary
   * analyses; the original spelling will be kept." */
  readings: ReviewReading[]
  /** The order to list readings in, the context reading first. */
  displayOrder: number[]
  contextReading: number | null
  /** Selected when the review opens; null = "Keep original spelling". */
  preselected: number | null
  lemmaAmbiguous: boolean
}

export interface AppliedReview {
  text: string
  lemmas: LemmaChoice[]
  /** False when the entry already has this text and these lemma choices. */
  changed: boolean
}

export interface Review {
  entryId: string
  /** The text reviewed; applyReview must be given it back. */
  original: string
  words: ReviewWord[]
  /** Set when no word is ambiguous: apply it directly (if changed) without
   * opening the dialog. */
  direct: AppliedReview | null
}

/** A reading index per review word, or null to keep its spelling. */
export type ReviewChoices = (number | null)[]

export type Request =
  | { type: 'setProject'; project: ProjectContent }
  | { type: 'setStructure'; lessons: LessonOutline[] }
  | { type: 'setEntry'; entry: EntryContent }
  | { type: 'removeEntry'; id: string }
  | { type: 'setL1Name'; l1Name: string }
  | { type: 'setGaps'; gaps: Gaps }
  | { type: 'search'; query: string; options: SearchOptions }
  | { type: 'firstUse' }
  | { type: 'spacedRepetition' }
  | { type: 'newInLesson'; lessonId: string }
  | { type: 'review'; entryId: string }
  | { type: 'applyReview'; entryId: string; original: string; choices: ReviewChoices }
  | { type: 'macronize'; text: string }

/** What each request answers. */
export interface Results {
  setProject: void
  setStructure: void
  setEntry: void
  removeEntry: void
  setL1Name: void
  setGaps: void
  search: SearchResult
  firstUse: FirstUses
  spacedRepetition: ReviewFlag[]
  newInLesson: Lemma[]
  review: Review | null
  applyReview: AppliedReview | null
  macronize: string
}

export interface RequestMessage {
  /** Matches the response to the request. */
  id: number
  /** The client's content revision when the request was sent. */
  revision: number
  request: Request
}

export type ResponseMessage =
  | { type: 'ready' }
  /** Startup failed (the wasm could not be loaded). */
  | { type: 'failed'; error: string }
  /** `json` is the result as JSON, or absent for requests without one. */
  | { type: 'result'; id: number; revision: number; json?: string }
  | { type: 'error'; id: number; revision: number; error: string; fatal: boolean }
