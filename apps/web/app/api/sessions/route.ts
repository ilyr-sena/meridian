import { NextResponse, type NextRequest } from "next/server"
import { getDb } from "@/lib/mongodb"
import { ObjectId } from "mongodb"
import type { MongoSession, MongoUser } from "@/lib/types"

const CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"

function generateSessionId(): string {
  let id = ""
  for (let i = 0; i < 6; i++) {
    id += CHARS[Math.floor(Math.random() * CHARS.length)]
  }
  return id
}

export async function GET() {
  try {
    const db = await getDb()

    const currentUser = await db.collection<MongoUser>("users").findOne()
    if (!currentUser) {
      return NextResponse.json({ sessions: [], activeSession: null })
    }

    const userId = String(currentUser._id)
    const userOid = new ObjectId(userId)

    const activeSession = await db
      .collection<MongoSession>("sessions")
      .findOne({ status: "active", user: userOid })

    const sessions = await db
      .collection<MongoSession>("sessions")
      .find({ user: userOid })
      .sort({ started: -1 })
      .toArray()

    const serializeSession = (s: MongoSession) => ({
      _id: String(s._id),
      id: s.id,
      user: String(s.user),
      devices: s.devices.map(String),
      devices_log: (s.devices_log ?? []).map((entry) => ({
        device: String(entry.device),
        added_at: entry.added_at,
        removed_at: entry.removed_at ?? null,
      })),
      status: s.status,
      started: s.started,
      ended: s.ended ?? null,
    })

    return NextResponse.json({
      sessions: sessions.map(serializeSession),
      activeSession: activeSession
        ? { ...serializeSession(activeSession), isActive: true }
        : null,
      currentUserId: userId,
    })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}

export async function POST(request: NextRequest) {
  try {
    const db = await getDb()
    const { deviceId } = await request.json()

    if (!deviceId || !ObjectId.isValid(deviceId)) {
      return NextResponse.json({ error: "Valid deviceId is required" }, { status: 400 })
    }

    const currentUser = await db.collection<MongoUser>("users").findOne()
    if (!currentUser) {
      return NextResponse.json({ error: "No users found" }, { status: 404 })
    }

    const userId = String(currentUser._id)
    const userOid = new ObjectId(userId)

    const existingActive = await db
      .collection<MongoSession>("sessions")
      .findOne({ status: "active", user: userOid })

    if (existingActive) {
      return NextResponse.json({ error: "User already has an active session" }, { status: 409 })
    }

    let sessionCode = generateSessionId()
    while (await db.collection<MongoSession>("sessions").findOne({ id: sessionCode })) {
      sessionCode = generateSessionId()
    }

    const newSession = {
      user: userOid,
      team: new ObjectId(String(currentUser.team)),
      devices: [new ObjectId(deviceId)],
      devices_log: [{ device: new ObjectId(deviceId), added_at: new Date() }],
      id: sessionCode,
      started: new Date(),
      status: "active" as const,
    }

    const result = await db.collection<MongoSession>("sessions").insertOne(newSession)

    return NextResponse.json({
      _id: String(result.insertedId),
      id: sessionCode,
      user: userId,
      team: String(currentUser.team),
      devices: [deviceId],
      devices_log: [{ device: deviceId, added_at: newSession.started.toISOString(), removed_at: null }],
      status: "active",
      started: newSession.started.toISOString(),
      ended: null,
    }, { status: 201 })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}
