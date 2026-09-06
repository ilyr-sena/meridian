import { MongoClient, type Db } from "mongodb"

const MONGODB_URI = process.env.MONGODB_URI!
const MONGODB_DB = process.env.MONGODB_DB ?? "meridian"

let cachedClient: MongoClient | null = null
let cachedDb: Db | null = null

export async function getDb(): Promise<Db> {
  if (cachedDb) return cachedDb

  cachedClient = new MongoClient(MONGODB_URI, {
    serverSelectionTimeoutMS: 10000,
  })
  await cachedClient.connect()
  cachedDb = cachedClient.db(MONGODB_DB)
  return cachedDb
}
