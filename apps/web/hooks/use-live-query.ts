"use client"

import { useCallback, useEffect, useRef, useState } from "react"

type UseLiveQueryOptions<T> = {
  url: string
  collection?: string
  transform?: (data: unknown) => T
  interval?: number
}

type ChangeListener = (collection: string) => void

const listeners = new Set<ChangeListener>()
let sharedEventSource: EventSource | null = null
let reconnectTimeout: ReturnType<typeof setTimeout> | null = null

function initSharedSSE() {
  if (typeof window === "undefined" || sharedEventSource) return

  try {
    const es = new EventSource("/api/events")
    sharedEventSource = es

    es.addEventListener("change", (e: MessageEvent) => {
      try {
        const payload = JSON.parse(e.data)
        const coll = payload.collection
        if (coll) {
          listeners.forEach((fn) => fn(coll))
        }
      } catch {
        // ignore malformed SSE frames
      }
    })

    es.addEventListener("error", () => {
      es.close()
      sharedEventSource = null
      if (!reconnectTimeout) {
        reconnectTimeout = setTimeout(() => {
          reconnectTimeout = null
          initSharedSSE()
        }, 5000)
      }
    })
  } catch {
    // SSE initialization error fallback
  }
}

export function useLiveQuery<T>({
  url,
  collection,
  transform,
  interval = 2000,
}: UseLiveQueryOptions<T>) {
  const [data, setData] = useState<T | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  const fetchData = useCallback(async () => {
    try {
      const res = await fetch(url)
      if (!res.ok) throw new Error(String(res.status))
      const json = await res.json()
      if (!mountedRef.current) return
      setData(transform ? transform(json) : (json as T))
      setError(null)
    } catch (e) {
      if (!mountedRef.current) return
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      if (mountedRef.current) setLoading(false)
    }
  }, [url, transform])

  useEffect(() => {
    mountedRef.current = true
    fetchData()

    // Initialize real-time SSE stream listener
    initSharedSSE()

    const onCollectionChange: ChangeListener = (changedColl) => {
      if (!mountedRef.current) return
      if (!collection || collection === changedColl) {
        fetchData()
      }
    }

    listeners.add(onCollectionChange)

    // Fallback interval (60s)
    const iv = setInterval(() => {
      if (mountedRef.current) fetchData()
    }, interval)

    return () => {
      mountedRef.current = false
      listeners.delete(onCollectionChange)
      clearInterval(iv)
    }
  }, [fetchData, collection, interval])

  return { data, loading, error, refetch: fetchData }
}
