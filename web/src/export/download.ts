import { ApiError } from '../api/client'
import type { ApiErrorBody } from '../api/types'
import { filenameFromContentDisposition } from './helpers'

/** Fetch a file and save it with the server's filename. Throws ApiError with
 * the server's message on failure. */
export async function downloadFile(url: string, fallbackName: string): Promise<string> {
  const response = await fetch(url, { headers: { 'X-Loquaris': '1' }, credentials: 'same-origin' })
  if (!response.ok) {
    let message = response.statusText || `Export failed (${response.status})`
    try {
      const body = (await response.json()) as ApiErrorBody
      if (body.error) message = body.error
    } catch {
      // Not JSON.
    }
    throw new ApiError(response.status, message)
  }
  const name = filenameFromContentDisposition(response.headers.get('Content-Disposition')) ?? fallbackName
  const blob = await response.blob()
  const href = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = href
  link.download = name
  document.body.append(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(href), 60_000)
  return name
}
