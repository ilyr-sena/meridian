import { NextResponse } from "next/server"
import { getDb } from "@/lib/mongodb"
import { ObjectId } from "mongodb"
import type { MongoDevice, MongoSession, MongoUser } from "@/lib/types"

export async function GET() {
  try {
    const db = await getDb()

    const currentUser = await db.collection<MongoUser>("users").findOne()
    if (!currentUser) {
      return NextResponse.json({ devices: [] })
    }

    const currentUserId = String(currentUser._id)
    const currentTeamId = String(currentUser.team)

    const activeSessions = await db
      .collection<MongoSession>("sessions")
      .find({ status: "active" })
      .toArray()

    const deviceUserMap = new Map<string, string>()
    for (const session of activeSessions) {
      if (session.ended) continue
      const uid = String(session.user)
      for (const deviceId of session.devices) {
        deviceUserMap.set(String(deviceId), uid)
      }
    }

    // Reconcile stale devices whose heartbeat expired (> 25s) or never arrived
    const expiredCutoff = new Date(Date.now() - 25000)
    await db.collection<MongoDevice>("devices").updateMany(
      {
        status: "online",
        $or: [
          { last_heartbeat: { $lt: expiredCutoff } },
          { last_heartbeat: { $exists: false } },
        ],
      },
      {
        $set: { status: "offline" },
      }
    )

    const devices = await db.collection<MongoDevice>("devices").find().toArray()

    const userObjectId = new ObjectId(currentUserId)
    const teamObjectId = new ObjectId(currentTeamId)

    const visibleDevices = devices.filter((d) => {
      const deviceTeam = String(d.team)
      if (deviceTeam !== currentTeamId) return false
      // If device has no user restrictions, all team members can use it
      if (!d.users || d.users.length === 0) return true
      // Otherwise, only listed users can use it
      return d.users.some((uid) => String(uid) === currentUserId)
    })

    const devicesWithStatus = visibleDevices.map((d) => {
      const id = String(d._id)
      const isOnline = d.status === "online"
      const rawInUseBy = deviceUserMap.get(id) ?? null
      const inUseBy = isOnline ? rawInUseBy : null

      let badge: "in_use" | "available" | "offline"
      if (!isOnline) {
        badge = "offline"
      } else if (inUseBy) {
        badge = "in_use"
      } else {
        badge = "available"
      }

      return {
        _id: id,
        name: d.name,
        model: d.model,
        version: d.version,
        status: d.status,
        badge,
        in_use_by: inUseBy,
        owned_by_current_user: isOnline && inUseBy === currentUserId,
        apps: d.apps,
        tailscale_ip: d.tailscale_ip,
        host_ports: d.host_ports,
      }
    })

    return NextResponse.json({ devices: devicesWithStatus })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}
