import type { ObjectId } from "mongodb"

export interface MongoUser {
  _id: ObjectId
  team: ObjectId
  name: string
  role: ObjectId
  permissions: string[]
}

export interface MongoTeam {
  _id: ObjectId
  name: string
}

export interface MongoDevice {
  _id: ObjectId
  name: string
  model: string
  version: string
  udid: string
  status: "online" | "offline"
  team: ObjectId
  users: ObjectId[]
  apps: string[]
  tailscale_ip?: string
  last_heartbeat?: Date
  host_ports?: {
    wda?: number
    bridge?: number
    stream?: number
  }
}

export interface DeviceLogEntry {
  device: ObjectId
  added_at: Date
  removed_at?: Date | null
}

export interface MongoSession {
  _id?: ObjectId
  user: ObjectId
  devices: ObjectId[]
  devices_log: DeviceLogEntry[]
  team: ObjectId
  started: Date
  ended?: Date | null
  status: "active" | "ended"
  end_reason?: string | null
  id: string
}

export interface MongoRole {
  _id: ObjectId
  name: string
  description: string
  system: boolean
}

export interface MongoPermission {
  _id: ObjectId
  key: string
  resource: string
  action: string
  description: string
}
