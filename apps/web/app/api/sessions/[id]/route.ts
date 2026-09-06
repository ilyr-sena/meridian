import { NextResponse, type NextRequest } from "next/server"
import { getDb } from "@/lib/mongodb"
import { ObjectId } from "mongodb"
import type { MongoSession } from "@/lib/types"

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params
    const body = await request.json()

    if (!ObjectId.isValid(id)) {
      return NextResponse.json({ error: "Invalid session ID" }, { status: 400 })
    }

    const db = await getDb()
    const session = await db
      .collection<MongoSession>("sessions")
      .findOne({ _id: new ObjectId(id) })

    if (!session) {
      return NextResponse.json({ error: "Session not found" }, { status: 404 })
    }

    if (session.status === "ended") {
      return NextResponse.json({ error: "Session already ended" }, { status: 409 })
    }

    // Swap device
    if (body.swap_device) {
      const newDeviceId = body.swap_device
      if (!ObjectId.isValid(newDeviceId)) {
        return NextResponse.json({ error: "Invalid device ID" }, { status: 400 })
      }

      const now = new Date()
      const newDeviceOid = new ObjectId(newDeviceId)

      // Mark current devices as removed in devices_log
      const logUpdates = session.devices.map((deviceOid) => ({
        updateOne: {
          filter: { _id: new ObjectId(id), "devices_log.device": deviceOid },
          update: { $set: { "devices_log.$.removed_at": now } },
        },
      }))

      if (logUpdates.length > 0) {
        await db.collection<MongoSession>("sessions").bulkWrite(logUpdates)
      }

      // Add new device to devices_log and update devices array
      await db
        .collection<MongoSession>("sessions")
        .updateOne(
          { _id: new ObjectId(id) },
          {
            $set: { devices: [newDeviceOid] },
            $push: {
              devices_log: { device: newDeviceOid, added_at: now, removed_at: null },
            },
          }
        )

      return NextResponse.json({
        _id: String(session._id),
        id: session.id,
        devices: [newDeviceId],
        status: "active",
      })
    }

    // End session
    if (body.end_reason !== undefined || body.status === "ended") {
      const now = new Date()
      await db
        .collection<MongoSession>("sessions")
        .updateOne(
          { _id: new ObjectId(id) },
          { $set: { status: "ended", ended: now, end_reason: body.end_reason ?? "user" } }
        )

      return NextResponse.json({
        _id: String(session._id),
        id: session.id,
        status: "ended",
        ended: now.toISOString(),
        end_reason: body.end_reason ?? "user",
      })
    }

    return NextResponse.json({ error: "No valid action provided" }, { status: 400 })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}
