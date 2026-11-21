// react-query wiring for the REST API: query keys, the client, and hooks for
// data used on several screens.
import { MutationCache, QueryCache, QueryClient, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from './client'
import type { User } from './types'

export const queryKeys = {
  me: ['me'] as const,
  config: ['config'] as const,
  projects: ['projects'] as const,
  project: (id: string) => ['project', id] as const,
  sessions: ['sessions'] as const,
  adminUsers: ['admin', 'users'] as const,
  invite: (token: string) => ['invite', token] as const,
}

function isUnauthorized(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401
}

export function createQueryClient(): QueryClient {
  // Any 401 means the session ended: forget the user, and RequireAuth sends
  // them to the sign-in page with a return path.
  const onError = (error: unknown) => {
    if (isUnauthorized(error)) client.setQueryData(queryKeys.me, null)
  }
  const client: QueryClient = new QueryClient({
    queryCache: new QueryCache({ onError }),
    mutationCache: new MutationCache({ onError }),
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        refetchOnWindowFocus: true,
        retry: (count, error) => !(error instanceof ApiError && error.status < 500) && count < 2,
      },
    },
  })
  return client
}

/** The signed-in user, or null when signed out. */
export function useMe() {
  return useQuery({
    queryKey: queryKeys.me,
    queryFn: async (): Promise<User | null> => {
      try {
        return await api.auth.me()
      } catch (error) {
        if (isUnauthorized(error)) return null
        throw error
      }
    },
    staleTime: 5 * 60_000,
  })
}

/** The signed-in user inside authenticated routes (RequireAuth guarantees it). */
export function useCurrentUser(): User {
  const { data } = useMe()
  if (!data) throw new Error('useCurrentUser outside RequireAuth')
  return data
}

export function useConfig() {
  return useQuery({ queryKey: queryKeys.config, queryFn: api.config, staleTime: 10 * 60_000 })
}

export function useSetMe() {
  const client = useQueryClient()
  return (user: User | null) => client.setQueryData(queryKeys.me, user)
}
