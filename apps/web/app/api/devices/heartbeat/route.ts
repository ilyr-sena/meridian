import { NextRequest, NextResponse } from "next/server"
import { getDb } from "@/lib/mongodb"
import { ObjectId } from "mongodb"
import type { MongoDevice, MongoTeam } from "@/lib/types"

export async function POST(req: NextRequest) {
  try {
    const body = await req.json()
    const {
      udid,
      name,
      model,
      version,
      status = "online",
      tailscale_ip,
      host_ports,
      apps,
    } = body

    if (!udid) {
      return NextResponse.json({ error: "missing udid" }, { status: 400 })
    }

    const db = await getDb()
    const devicesCol = db.collection<MongoDevice>("devices")

    // Match by UDID or name fallback
    let device = await devicesCol.findOne({ udid })
    if (!device && name) {
      device = await devicesCol.findOne({ name })
    }

    const now = new Date()

    if (device) {
      const updateDoc: Record<string, unknown> = {
        status,
        last_heartbeat: now,
        udid, // sync actual UDID if matched by name
      }
      if (model) updateDoc.model = model
      if (version) updateDoc.version = version
      if (tailscale_ip) updateDoc.tailscale_ip = tailscale_ip
      if (host_ports !== undefined) updateDoc.host_ports = host_ports
      if (Array.isArray(apps) && apps.length > 0) updateDoc.apps = apps
      if (!device.team) {
        const teamsCol = db.collection<MongoTeam>("teams")
        const team = await teamsCol.findOne()
        if (team) updateDoc.team = team._id
      }

      await devicesCol.updateOne(
        { _id: device._id },
        { $set: updateDoc }
      )

      // Terminate any active session if explicitly requested or device went offline / stopped session
      if (body.end_active_session || body.session_state === "ended" || status === "offline") {
        await db.collection("sessions").updateMany(
          { devices: device._id, status: "active" },
          { $set: { status: "ended", ended: now, end_reason: body.end_reason || "hub_stopped" } }
        )
      }

      return NextResponse.json({
        success: true,
        deviceId: String(device._id),
        status,
      })
    }

    // New device: find default team
    const teamsCol = db.collection<MongoTeam>("teams")
    const team = await teamsCol.findOne()
    const teamId = team ? team._id : new ObjectId()

    const newDevice: Partial<MongoDevice> = {
      name: name || model || "iPhone",
      model: model || "iPhone",
      version: version || "iOS",
      udid,
      status,
      team: teamId,
      users: [],
      apps: Array.isArray(apps) ? apps : [],
      tailscale_ip,
      host_ports: host_ports ?? null,
      last_heartbeat: now,
    }

    const res = await devicesCol.insertOne(newDevice as MongoDevice)
    return NextResponse.json({
      success: true,
      deviceId: String(res.insertedId),
      created: true,
      status,
    })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}
