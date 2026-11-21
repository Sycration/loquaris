# Desktop feature inventory (parity spec)

What the desktop app (commit `5877237`, `src/` in that commit)
does, so the web rebuild loses nothing. File references are relative to
that `src/` (`git show 5877237:src/<file>`). Where the web app deliberately differs, DESIGN.md
("Changes from the desktop app") says so; otherwise this is the behaviour to
reproduce.

## 1. Shell and project lifecycle

- Fonts: IBM Plex Sans for UI and L1 text (15 pt); Source Serif 4 for Latin.
  Dark theme.
- Desktop had New / Open / Save / Save as / Close and a recent-projects list,
  an unsaved-changes indicator and save prompts. (Web: autosave, a project
  list, no save prompts.)
- New project: empty name, L1 "English", one empty expanded lesson; the
  project details open with the name focused.
- Validation: at least one lesson; student entries must be Latin; gaps
  0–60 s; PDF font size 8–30 (`project.rs:200-218`).

## 2. Main layout (`ui/mod.rs`, `ui/chrome.rs`)

- Script on the left two thirds, side panel on the right third.
- Toolbar: "+ New lesson", Undo, Redo; column headings "Teacher" (sans, 30 pt)
  and "Student" (Latin serif, 30 pt).
- Script: two columns, teacher left, student right, split by a vertical line.
  Off-screen rows are virtualized.
- Add-entry bar at the bottom: "Adding to lesson N: {name}"; teacher side
  buttons "+ {L1 name}" (teal) and "+ Latin" (red); student side "+ Latin".
- Double-clicking empty background clears the selection and stops editing.

## 3. Lessons (`ui/lesson.rs`, `ui/script.rs`)

- Header: chevron to collapse/expand; inline name field with a bold
  "Lesson N: " prefix and hint "Lesson name"; play button (▶ Play lesson /
  ❚❚ Pause lesson / ▶ Resume lesson), disabled while recording or when the
  lesson is empty; right-aligned recording summary (12 pt).
- Clicking the header makes the lesson active and clears the selection.
- Recording summary (`model.rs:77-107`): "Recordings incomplete" if empty or
  any entry lacks a clip; otherwise total of cropped durations plus gaps,
  `MM:SS.t`, or `H:MM:SS.t` from one hour.
- Footer bar "Lesson N: {name or Untitled}" with an up chevron that collapses.
- Sticky header: the header of the lesson crossing the top of the viewport is
  repeated there and fully works.
- "+ New lesson" appends an empty lesson, scrolls to it, focuses its name.
- Active lesson (target for new entries and the side panel): the selection's
  lesson, a clicked header/body, or the lesson crossing the viewport top
  while scrolling.

## 4. Entry cards (`ui/card.rs`)

- One card per row, in its participant's column.
- Colours: L1 fill rgb(24,68,80) accent rgb(60,184,213); Latin fill
  rgb(80,0,0) accent rgb(224,70,70).
- Language label (L1 name or "Latin", 12 pt, accent colour), left-aligned for
  teacher, right-aligned for student.
- Text 15 pt (Latin serif or sans); placeholder "Enter text…".
- Selected: fill brightened ×1.35 and a white 1 px border.
- Audio marker: a 4 px lime strip rgb(189,219,63) on the centre-facing side
  when a clip is attached.
- Hover buttons: ✎ "Edit entry"; speaker ("Record or import audio" when no
  clip, accent-coloured "Edit attached audio" with one) opens the recorder.
- First-use lemmas: lemmas first used in the project in this entry, yellow,
  Latin serif 11 pt, joined by " · "; teacher cards top-right, student cards
  top-left.
- Search matches in text: yellow, underlined.
- Editing starts with ✎, Cmd+E, or adding an entry; ends with Escape, blur,
  or any non-text command.
- Selecting text in a non-editing card (drag, double- or triple-click) runs a
  search: trimmed of surrounding non-alphanumerics; ignored if multi-line,
  longer than 100 characters, or empty.

## 5. Context menu (right-click a card)

- Single entry (or an entry outside the multi-selection, which reselects it):
  Duplicate; Auto-macronize (Latin only); "Move to student →" or "← Move to
  teacher"; "Switch to Latin" / "Switch to L1" (teacher only); Move up / Move
  down (disabled at the very first/last position of the script); Delete (no
  confirmation).
- Multi-selection: Duplicate selected; Move to student (if any teacher
  selected); Move to teacher (if any student); Switch to Latin (if any teacher
  L1); Switch to L1 (if any teacher Latin); Move up / down (only when
  contiguous); Delete selected.

## 6. Editing semantics (`app/editing.rs`)

- Add inserts a blank entry after the primary selection (in its lesson), or
  at the end of the active lesson; the lesson expands; the new entry is
  selected and edited.
- Students only speak Latin: teacher → student forces Latin; student →
  teacher stays Latin; switching language only for teacher entries.
- Move up/down: one place within the lesson; from a lesson's first entry, Move
  up goes to the end of the previous lesson; from the last, Move down goes to
  the start of the next.
- Bulk move (contiguous selection): the block swaps with its neighbour,
  crossing lesson boundaries the same way, order preserved.
- Duplicate inserts a copy (with audio, crop and lemma choices) after each
  target and selects the copies.
- Bulk operations are one undo step each.

## 7. Selection (`app/selection.rs`)

- Primary entry, set of extra selected entries, and a range anchor, in script
  order across lessons.
- Click: select only that entry, set anchor. Cmd-click: add (not toggle), set
  anchor. Alt-click: remove. Shift-click: range from anchor.
- Cmd+↑ / Cmd+↓: previous / next entry across lessons, expanding its lesson;
  with nothing selected, first / last entry of the active lesson.
- Revealing an entry from the side panel selects it and smoothly centres it.
- Selecting a different entry stops lesson playback.
- Escape: stops editing if editing, otherwise deselects and stops playback.

## 8. Undo / redo (`app/history.rs`)

- Undoable: gaps, export settings, audio and crop, text of project name / L1
  name / lesson name / entry text, insert lesson, insert/delete entry,
  participant/language, move, parsing review (text + lemma choices), bulk ops.
- Not undoable: selection, expand/collapse, search, keybinds.
- Refused while recording: "Stop recording before undo or redo."
- Structural changes close the recorder; changes affecting recordings stop
  lesson playback.

## 9. Lesson playback (`app/playback.rs`, `lesson_player.rs`)

- Starts at the selected entry if it is in the lesson, else the first.
- Plays cropped clips in order with gaps: same participant 100 ms, different
  500 ms by default (0–60 s).
- Stops at the first entry without a clip: "Playback stopped: entry N has no
  recording." (or "Entry N has no recording." if it is the starting entry).
- The selection follows playback (next entry selected when the previous clip
  ends); the lesson expands and scrolls to it.
- The play button toggles pause/resume; playing another lesson stops the
  current one; selecting another entry or changing recordings stops playback.

## 10. Recorder (`audio.rs`, `waveform.rs`)

- Input device chooser ("System default" first) with refresh; disabled while
  recording.
- Actions: Undo, Redo, "Import WAV…" (hidden while recording), "Delete clip",
  cropped length `MM:SS.hh` ("Length of the cropped clip").
- Bottom: Close; ▶/❚❚ Play/Pause, "|◀" Back to start, red ● Record / ■ Stop.
  Play and back disabled without a clip or while recording.
- Diagnostics: callbacks, samples, peak %, elapsed, Hz, channels; warnings
  "The stream started but has not received any audio callbacks." / "Audio
  samples are arriving, but they are silent."; copy diagnostics.
- Shows the entry: "Teacher/Student · language" and its text.
- A new take replaces the clip and clears the crop. "No audio was recorded."
  if empty. Takes are capped at 10 minutes: "Recording exceeded the 10 minute
  limit." and the take is discarded.
- Each take, import, delete, or crop-handle release is one undoable change.
- Closing while recording stops and keeps the take.
- Import: WAV only, up to 100 MiB; fully decoded to validate; original bytes
  stored unchanged (any rate, depth, float). Toast "Imported the WAV file."
- Recorder playback plays only the crop, from the cursor (or crop start if
  the cursor is outside); stop returns to crop start; ±5 s, Home, End seek
  clamped to the crop.
- Recorder keys: Space play/pause, R record/stop, S stop playback, ← / → seek
  5 s, Home / End. Key repeat ignored for play/record/stop.

## 11. Waveform and crop

- Peaks: min/max across channels per 1/400 s block; each screen column at
  least 5 blocks. Top and bottom halves stretched independently (display
  only).
- While recording: live waveform grows to fill a 10 s window, then shrinks to
  keep the whole take visible; cursor at the end with elapsed time.
- Otherwise: whole clip with outside-crop darkened; two handles just outside
  the crop ("Drag to choose where the clip starts" / "…ends") with times
  `MM:SS.hh` above, pushed apart if close; red playhead with time below;
  press or drag to seek.
- Crop: start and end frame (end exclusive), whole clip kept; at least 50 ms
  wide; a crop covering the whole clip is stored as none; applied on handle
  release; out-of-range crops clamped keeping at least one frame.
- Playing or exporting a crop applies a 10 ms gain ramp at any edge that cuts
  into the clip (not at the clip's own ends).

## 12. Project details

- Same participant gap (s, 0–60, 3 decimals, step 0.01, default 0.100);
  different participant gap (default 0.500); project name; L1 language
  (default "English"). All undoable; gap changes stop playback.

## 13. Toasts (`toasts.rs`)

- Bottom-right stack, 340 px wide, 16 px margin. Info ℹ (grey) and Success ✔
  (green) expire after 4 s (timer pauses while hovered); Error ⚠ (red) stays
  until dismissed. At most 5; a repeat of a visible message moves to the
  bottom with "×N". Each has Copy and × buttons.

## 14. Keybindings (`keybinds.rs`)

"Cmd" = Ctrl on Linux/Windows, Cmd on macOS. Exact modifier match only.

| Action | Default | Scope |
|---|---|---|
| Undo | Cmd+Z | shared |
| Redo | Cmd+Shift+Z | shared |
| Redo (alternative) | Cmd+Y | shared |
| Edit selected entry | Cmd+E | main |
| Duplicate selected entries | Cmd+D | main |
| Stop editing entry | Escape | main, while editing |
| Deselect entries | Escape | main, not editing |
| Select previous / next entry | Cmd+↑ / Cmd+↓ | main |
| Add teacher L1 / teacher Latin / student Latin | Cmd+J / Cmd+K / Cmd+L | main |
| Switch entry language | Cmd+R | main |
| Move to student / to teacher | Cmd+→ / Cmd+← | main |
| Move entry up / down | Cmd+Shift+↑ / Cmd+Shift+↓ | main |
| Play / pause | Space | recorder |
| Start / stop recording | R | recorder |
| Stop playback | S | recorder |
| Seek back / forward 5 s | ← / → | recorder |
| Seek to beginning / end | Home / End | recorder |

(Desktop also had Open Cmd+O, Save Cmd+S, Save as Cmd+Shift+S.)

- Editor: table of actions with a Recorder section; click a shortcut, "Press
  a shortcut…", next key press binds, Escape cancels; "Restore defaults".
- Validation: main actions need Ctrl/Cmd, except Escape alone for Stop
  editing / Deselect; recorder actions may be bare keys but not Escape ("Use
  Ctrl or Command with a key."). Conflicts by scope: shared conflicts with
  all; main and recorder don't conflict with each other; Stop editing and
  Deselect may share ("That shortcut is already assigned.").
- Main shortcuts are off while parsing review or the recorder is open, or
  while capturing a key.

## 15. Search (`search.rs`, `ui/side_panel.rs`)

Side panel, top to bottom: Search, Analysis, "New in lesson N", "Uses per
lesson", "Contexts in lesson N".

- Search box ("Search the project…") with ✕ clear; "Exact text" and
  "Unmarked vowels are short" checkboxes (session options, default off).
- Every edit re-parses the query with collo, keeping readings that fit the
  written vowel marks.
- Lemma mode if the trimmed query is a single recognised word (default lemma:
  highest frequency among its readings); otherwise text mode. "Exact text"
  forces text mode and stays on until a lemma is picked, which turns it off.
- Text matching: case- and accent-insensitive (NFD, marks stripped,
  lowercased), plain substring, non-overlapping, all entries (L1 and Latin),
  highlights mapped back to the original text.
- Lemma matching: Latin entries only; a word matches if any reading fitting
  its marks has the lemma (stricter set with unmarked-short); a word with a
  chosen lemma matches only that lemma; hits sorted by lesson, entry,
  position.
- Unmarked vowels are short: an unmarked vowel is read short whenever a
  reading with it short exists ("malae" → mălus, not māla).
- Summary: "N uses of the lemma {headword}" or "N uses of this text", plus
  " in M lessons".
- Analysis: per query word, the word (serif 16 pt); "K other readings ruled
  out by vowel length" (tooltip depends on the option); each lemma as reader
  form headword plus translation in the L1; for a single-word query, lemmas
  are clickable ("Search every form of this lemma"); under each lemma its
  readings "★ form — morphology", ★ marking collo's context choice ("Most
  likely reading in context"). Morphology in fr/es if L1 is French/Spanish,
  else en. "Not recognised as Latin." for unknown words.
- L1 name → translation language: French/français/francais/fr → fr;
  German/deutsch → de; Spanish/español → es; Italian → it; Portuguese → pt;
  Catalan → ca; Galician/galego → gl; else en (`search.rs:296-307`).
- New in lesson N: lemmas whose first use in the project is in the active
  lesson, as clickable chips in script order (hover: translation; click:
  search that lemma). Empty: "No Latin words are introduced in this lesson."
  A word's lemma is its chosen lemma, else the highest-frequency lemma among
  readings fitting its marks. Same data drives card first-use labels.
- Uses per lesson: one column per lesson; teacher counts as bars above a
  middle line (#3987e5), student below (#d95926), shared scale; axis max, 0,
  max; labels "Lesson 1" and the last number; current and hovered lesson
  shaded; tooltip "Lesson N: name / Teacher x · Student y / Click to go to
  the first use"; click reveals the first hit in that lesson, or scrolls to
  the lesson.
- Contexts in lesson N: each entry of the active lesson with hits,
  highlighted (serif 14 pt), with "→" ("Select this entry in the script").
  Empty: "Search to see each use in this lesson." / "Not used in this
  lesson."

## 16. Parsing review (`parsing_review.rs`)

- Started by Auto-macronize on a single Latin entry.
- The entry is tagged with collo; each word starts at collo's context
  reading (if alignable); saved choices then preselect (if the lemma has a
  non-conflicting, alignable reading).
- If unambiguous (every word has one lemma across all readings and all
  readings give the same alignable spelling), it applies directly (nothing if
  no change). Any lemma ambiguity opens the review, even with a saved choice.
- Modal "Review Latin parsings": note "The reading selected from context is
  marked; other readings are not ranked."; "Choose a word:" chips with
  previewed spellings; "Readings for X": "Keep original spelling", then each
  reading (context one first, " · selected from context"), "spelling —
  headword · morphology"; unalignable readings disabled ("This reading has a
  different spelling, so it cannot be applied safely."); unknown words: "No
  dictionary analyses; the original spelling will be kept."; Apply / Cancel.
- Apply is one undoable change of text plus lemma choices.
- Stored choices: token (collo-normalized, lowercased spelling), occurrence
  (index among same-spelling words in the entry, from 0), lemma key,
  extension flag. Saved only for words with more than one possible lemma and
  a chosen reading; "Keep original spelling" saves nothing. Ignored when the
  lemma is not among readings fitting the marks.

## 17. Auto-macronize (`macronize.rs`)

- Align written word and collo form+suffix as decomposed letters with marks
  (Cyrillic ў fixed to y); counts must match and letters equal after
  normalize+lowercase, else keep the user's spelling.
- Diphthongs ae/oe/au/eu: macron on the first vowel removed.
- Long by position (two+ consonants, x/z count two, h ignored, qu's u
  skipped): no macron inferred.
- Replace mode (macronize/review): on vowels where collo has a mark, remove
  the user's macron/breve first. Add a macron where collo has one and the
  vowel is unmarked. Breves never written. Case preserved, NFC output.
- `quantity_match`: Conflict / MarksMatch / Exact (see source).
  `matching_analyses`: drop conflicts (or keep best level with
  unmarked-short); if all conflict keep all. `reader_form`: only the macrons
  a reader expects.

## 18. Export (`export_pdf.rs`, `export_wav.rs`)

PDF:
- Paper A4 (default) / Letter / Legal / A3; font size 8–30, step 0.5, default
  15; all sizes scale by size/15.
- Glyph check before building: "Cannot export {field}: the bundled font does
  not support 'c' (U+XXXX)."
- Plex Sans except Latin entries and the "Latin" label (Source Serif 4).
  36 pt margins, black text.
- Every page: project name (sans 14 pt, max 2 lines) if set; "Teacher" at
  the left margin and "Student" at page centre + 8 (sans 18 pt).
- Every lesson starts a new page; light-grey (0.9) full-width header bar,
  22 pt tall, "Lesson N: {name}" sans 12 pt (wraps into more bars; an empty
  name shows "Lesson N: "); the bar repeats after the entries.
- Entries: filled box in the teacher or student column; column width =
  (page width − 72)/2, inset 8, box width = column − 16; fill (1,.91,.91)
  Latin, (.88,.96,.98) L1; language label 12 pt (max 3 lines, "…"), text
  15 pt, 19 pt leading; wrap at spaces or characters; tabs → 4 spaces; an
  entry that doesn't fit splits across pages; new page starts with "Lesson N
  (continued)".

WAV:
- Scope: entire project or one lesson; include teacher / student audio (both
  on). Only when every lesson in scope is fully recorded ("All selected
  lessons must be fully recorded.").
- One file per lesson "{project stem} - {lesson stem}.wav" (unnamed lesson
  stem "project"); duplicate names (case-insensitive) are an error.
- 48 kHz, stereo, 16-bit PCM; crop resampled by linear interpolation; mono
  copied to both channels; >2 channels: even → left, odd → right, averaged;
  10 ms crop ramps; gaps as in playback, no leading gap; an excluded
  participant's clips become silence of the same length; clamp and round, no
  normalization.

## 19. Audio processing

- Recording: device rate and channel count, converted to 16-bit; on stop a
  10 Hz high-pass (DC removal) per channel, then 250 ms linear fade in and
  out (overlapping for short clips); stored as 16-bit PCM WAV.
- Imports stored byte-for-byte. No normalization anywhere.
