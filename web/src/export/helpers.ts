// Pure helpers for the export dialog.

/** The filename from a Content-Disposition header: RFC 5987 `filename*`
 * wins over plain `filename`. Null when there is none. */
export function filenameFromContentDisposition(header: string | null | undefined): string | null {
  if (!header) return null
  const extended = /filename\*\s*=\s*([^;]+)/i.exec(header)
  if (extended) {
    const value = extended[1].trim()
    const match = /^([^']*)'[^']*'(.*)$/.exec(value)
    const encoded = match ? match[2] : value
    try {
      const name = decodeURIComponent(encoded.replace(/^"|"$/g, ''))
      if (name) return sanitizeFilename(name)
    } catch {
      // Malformed escapes: fall back to the plain parameter.
    }
  }
  const plain = /filename\s*=\s*(?:"((?:[^"\\]|\\.)*)"|([^;]+))/i.exec(header)
  if (plain) {
    const name = plain[1] !== undefined ? plain[1].replace(/\\(.)/g, '$1') : plain[2].trim()
    if (name) return sanitizeFilename(name)
  }
  return null
}

/** Drop any directory part so a header can't pick a path. */
function sanitizeFilename(name: string): string {
  return name.split(/[\\/]/).pop()?.trim() || 'download'
}

export interface LessonAudioInfo {
  id: string
  name: string
  /** One flag per entry: has a recording. */
  recorded: boolean[]
}

/** A lesson is exportable as WAV when it has entries and all are recorded. */
export function lessonComplete(lesson: LessonAudioInfo): boolean {
  return lesson.recorded.length > 0 && lesson.recorded.every(Boolean)
}

/** Names of lessons in scope that are not fully recorded. */
export function incompleteLessons(lessons: readonly LessonAudioInfo[]): LessonAudioInfo[] {
  return lessons.filter((lesson) => !lessonComplete(lesson))
}

export const INCOMPLETE_MESSAGE = 'All selected lessons must be fully recorded.'

export const PAPER_SIZES = [
  { value: 'a4', label: 'A4' },
  { value: 'letter', label: 'Letter' },
  { value: 'legal', label: 'Legal' },
  { value: 'a3', label: 'A3' },
] as const

/** 8–30 in steps of 0.5. */
export const FONT_SIZES: number[] = Array.from({ length: 45 }, (_, i) => 8 + i * 0.5)
