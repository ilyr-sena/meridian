import { getDb } from "@/lib/mongodb"

const COLLECTIONS = ["devices", "sessions", "users", "teams", "roles"]

export async function GET() {
  const encoder = new TextEncoder()
  let closed = false

  const stream = new ReadableStream({
    async start(controller) {
      function send(event: string, data: unknown) {
        if (closed) return
        try {
          controller.enqueue(encoder.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`))
        } catch {
          closed = true
        }
      }

      send("connected", { collections: COLLECTIONS })

      let changeStream: ReturnType<import("mongodb").Db["watch"]> | null = null
      let keepAlive: ReturnType<typeof setInterval> | null = null

      try {
        const db = await getDb()
        changeStream = db.watch([], { fullDocument: "updateLookup" })

        changeStream.on("change", (change: { ns?: { coll: string }; operationType: string; documentKey?: { _id: unknown }; fullDocument?: unknown }) => {
          const coll = change.ns?.coll
          if (!coll || !COLLECTIONS.includes(coll)) return
          if (!change.documentKey) return
          send("change", {
            collection: coll,
            operation: change.operationType,
            documentKey: change.documentKey,
            fullDocument: change.fullDocument ?? null,
          })
        })

        changeStream.on("error", (err: Error) => {
          send("error", { message: err.message })
        })

        keepAlive = setInterval(async () => {
          send("ping", { ts: Date.now() })
          try {
            const cutoff = new Date(Date.now() - 25000)
            await db.collection("devices").updateMany(
              {
                status: "online",
                $or: [
                  { last_heartbeat: { $lt: cutoff } },
                  { last_heartbeat: { $exists: false } },
                ],
              },
              { $set: { status: "offline" } }
            )
          } catch {
            // ignore TTL check error
          }
        }, 10000)
      } catch (e) {
        send("error", { message: e instanceof Error ? e.message : String(e) })
      }

      const cleanup = () => {
        closed = true
        if (keepAlive) clearInterval(keepAlive)
        if (changeStream) changeStream.close().catch(() => {})
        try { controller.close() } catch { /* already closed */ }
      }

      // Expose cleanup so the cancel path can call it
      const g = globalThis as Record<string, unknown>
      if (!g.__sseCleanups) g.__sseCleanups = new Set<() => void>()
      ;(g.__sseCleanups as Set<() => void>).add(cleanup)
    },

    cancel() {
      closed = true
      const cleanups = (globalThis as Record<string, unknown>).__sseCleanups as Set<() => void> | undefined
      if (cleanups) {
        for (const fn of cleanups) fn()
        cleanups.clear()
      }
    },
  })

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
    },
  })
}
