import { NextResponse } from "next/server"
import { getDb } from "@/lib/mongodb"
import { ObjectId } from "mongodb"
import type { MongoUser, MongoTeam, MongoRole } from "@/lib/types"

export async function GET() {
  try {
    const db = await getDb()

    const user = await db.collection<MongoUser>("users").findOne()
    if (!user) {
      return NextResponse.json({ error: "No users found" }, { status: 404 })
    }

    const [team, role] = await Promise.all([
      user.team
        ? db.collection<MongoTeam>("teams").findOne({ _id: new ObjectId(String(user.team)) })
        : null,
      user.role
        ? db.collection<MongoRole>("roles").findOne({ _id: new ObjectId(String(user.role)) })
        : null,
    ])

    return NextResponse.json({
      user: {
        _id: String(user._id),
        name: user.name,
        team: team ? { _id: String(team._id), name: team.name } : null,
        role: role ? { _id: String(role._id), name: role.name } : null,
      },
    })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    return NextResponse.json({ error: msg }, { status: 500 })
  }
}
