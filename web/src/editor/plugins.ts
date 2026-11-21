// Editor plugins: modules that register features through editorExtensions
// (see ./extensions.tsx). EditorPage imports this file, so importing a
// feature module here is all it takes to plug it in, e.g.
//
//   import '../audio/register'      // recorder, lesson playback
//   import '../analysis/register'   // search panel, first-use labels, macronize
//   import '../export/register'     // export dialog
//   import '../history/register'    // version history
import '../export/register'
import '../history/register'
import '../analysis/register'
import '../audio/register'
export {}
