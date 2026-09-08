"use client"

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import { motion, AnimatePresence } from "motion/react"
import {
  Activity01Icon,
  AppStoreIcon,
  Circle,
  CircleChevronDownIcon,
  CircleXIcon,
  FocusIcon,
  Home03Icon,
  KeyboardIcon,
  LoaderCircle,
  LockOpen,
  LockPasswordIcon,
  PauseCircleIcon,
  Plus,
  Settings01Icon,
  SmartPhoneIcon,
  VolumeHighIcon,
  VolumeLowIcon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import Image from "next/image"
import { cn } from "@workspace/ui/lib/utils"
import { H264StreamPlayer, StreamStats } from "./h264-stream-player"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@workspace/ui/components/popover"
import { Slider } from "@workspace/ui/components/slider"
import {
  Carousel,
  CarouselContent,
  CarouselItem,
  type CarouselApi,
} from "@workspace/ui/components/carousel"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip"
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@workspace/ui/components/drawer"
import { Label } from "@workspace/ui/components/label"
import {
  RadioGroup,
  RadioGroupItem,
} from "@workspace/ui/components/radio-group"
import { Button } from "@workspace/ui/components/button"
import { Badge } from "@workspace/ui/components/badge"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@workspace/ui/components/empty"
import { useLiveQuery } from "@/hooks/use-live-query"

const STREAM_BASE =
  process.env.NEXT_PUBLIC_IUS_STREAM_BASE ?? "http://localhost:9200"
const STREAM_WS =
  process.env.NEXT_PUBLIC_IUS_STREAM_WS ?? "ws://localhost:9200/stream.ws"
const WDA_BASE =
  process.env.NEXT_PUBLIC_IUS_WDA_BASE ?? "http://localhost:8100"
const CONTROL_WS =
  process.env.NEXT_PUBLIC_IUS_CONTROL_WS ?? "ws://localhost:9001/ws"
const CONTROL_HTTP =
  process.env.NEXT_PUBLIC_IUS_CONTROL_HTTP ?? "http://localhost:9001"

let globalWdaSessionId: string | null = null

async function getWdaSession(wdaBase: string = WDA_BASE): Promise<string | null> {
  if (globalWdaSessionId) return globalWdaSessionId
  try {
    const res = await fetch(`${wdaBase}/session`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ capabilities: {} }),
    })
    const data = await res.json()
    globalWdaSessionId = data?.value?.sessionId ?? null
    return globalWdaSessionId
  } catch {
    return null
  }
}

/** Display-side softening (CSS px) — masks aliasing baked in by server downscale.
 *  Kept as a fallback knob; the Swift pipeline now does Lanczos+unsharp, so 0. */
const STREAM_SOFTNESS_PX = 0

const RESOLUTION_OPTIONS = [
  { scale: 0.5, label: "540p" },
  { scale: 0.75, label: "720p" },
  { scale: 1.0, label: "1080p" },
  { scale: 1.25, label: "1440p" },
  { scale: 1.5, label: "4K" },
] as const

function getResolutionLabel(scale: number): string {
  const opt = RESOLUTION_OPTIONS.find((o) => o.scale === scale)
  return opt?.label ?? `${Math.round(scale * 1080)}p`
}

function buildStreamUrl(base: string, quality: number, fps: number, scale: number): string {
  const qMap = { 1: 50, 2: 75, 3: 90 }
  const q = qMap[quality as keyof typeof qMap] ?? 75
  return `${base}/stream?scale=${scale}&fps=${fps}&q=${(q / 100).toFixed(2)}`
}

type StreamStatus = "connecting" | "loaded" | "error"

type AppState = "running" | "stopped" | "closing"

type App = {
  name?: string
  iconUrl: string
  state: AppState
  bundleId?: string
  pid?: number | null
}

type RemoteApp = { bundleId: string; name: string }

type RunningApp = { bundleId: string; pid: number }

const APPS_PER_PAGE = 12

function chunkPages(list: App[]): App[][] {
  return list.reduce<App[][]>((pages, app, index) => {
    const pageIndex = Math.floor(index / APPS_PER_PAGE)
    ;(pages[pageIndex] ??= []).push(app)
    return pages
  }, [])
}

const AppIcon = memo(function AppIcon({
  app,
  onSelect,
}: {
  app: App
  onSelect: (app: App) => void
}) {
  const icon = (
    <div
      onClick={() => onSelect(app)}
      className="relative flex cursor-pointer flex-col items-center justify-center gap-0.5 transition-opacity duration-75 ease-in-out select-none active:opacity-70"
    >
      {/* eslint-disable-next-line @next/next/no-img-element -- icons are served by the local IUS bridge, not an optimizable host */}
      <img
        alt={app.name ?? ""}
        src={app.iconUrl}
        width={24}
        height={24}
        draggable={false}
        className="h-10 w-10 rounded-[23%] object-cover select-none"
        onError={(e) => {
          ;(e.currentTarget as HTMLImageElement).style.visibility = "hidden"
        }}
      />
      {app.state !== "stopped" && (
        <div className="absolute flex h-10 w-10 items-center justify-center rounded-[9px] bg-secondary/10 text-center backdrop-blur-md">
          {app.state === "running" ? (
            <HugeiconsIcon
              icon={PauseCircleIcon}
              size={18}
              strokeWidth={2}
              className="drop-shadow-lg drop-shadow-black/25"
            />
          ) : (
            <HugeiconsIcon
              icon={LoaderCircle}
              size={18}
              strokeWidth={2.5}
              className="rotate-45 animate-spin drop-shadow-lg drop-shadow-black/25"
            />
          )}
        </div>
      )}
    </div>
  )

  return app.name ? (
    <Tooltip>
      <TooltipTrigger>{icon}</TooltipTrigger>
      <TooltipContent side="bottom" className="select-none">
        <p>{app.name}</p>
      </TooltipContent>
    </Tooltip>
  ) : (
    icon
  )
})

function AppsCarousel({
  apps,
  onAppClick,
  isLoading,
  error,
  onRetry,
}: {
  apps: App[]
  onAppClick: (app: App) => void
  isLoading: boolean
  error: string | null
  onRetry: () => void
}) {
  const appPages = chunkPages(apps)
  const [carouselApi, setCarouselApi] = useState<CarouselApi | null>(null)
  const [currentPage, setCurrentPage] = useState(0)
  const [isDragging, setIsDragging] = useState(false)
  const dragStartRef = useRef<{ x: number; y: number } | null>(null)

  useEffect(() => {
    if (!carouselApi) return
    const onSelect = () => setCurrentPage(carouselApi.selectedScrollSnap() ?? 0)
    carouselApi.on("select", onSelect)
    carouselApi.on("reInit", onSelect)
    return () => {
      carouselApi.off("select", onSelect)
      carouselApi.off("reInit", onSelect)
    }
  }, [carouselApi])

  return (
    <div className="flex h-full w-fit flex-col gap-3.5 pt-1 pb-1">
      <div className="flex items-center justify-between pl-4 select-none">
        <span className="text-xs font-medium">Apps</span>
      </div>
      <div
        className="w-fit"
        onPointerDownCapture={(event) => {
          dragStartRef.current = {
            x: event.clientX,
            y: event.clientY,
          }
        }}
        onPointerMoveCapture={(event) => {
          const start = dragStartRef.current
          if (
            start &&
            !isDragging &&
            (Math.abs(event.clientX - start.x) > 6 ||
              Math.abs(event.clientY - start.y) > 6)
          ) {
            setIsDragging(true)
          }
        }}
        onPointerUpCapture={() => {
          dragStartRef.current = null
          setIsDragging(false)
        }}
        onPointerCancelCapture={() => {
          dragStartRef.current = null
          setIsDragging(false)
        }}
      >
        <Carousel setApi={setCarouselApi} className="w-54">
          <CarouselContent className="-ml-0 cursor-grab active:cursor-grabbing">
            {isLoading ? (
              <CarouselItem className="pl-0">
                <div className="grid w-full grid-cols-4 gap-2 px-4">
                  {Array.from({ length: APPS_PER_PAGE }).map((_, i) => (
                    <div key={i} className="flex flex-col items-center gap-1.5">
                      <div className="h-24 w-24 animate-pulse rounded-[23%] bg-white/10" />
                      <div className="h-2 w-12 animate-pulse rounded bg-white/10" />
                    </div>
                  ))}
                </div>
              </CarouselItem>
            ) : error ? (
              <CarouselItem className="pl-0">
                <div className="flex h-[7.75rem] flex-col items-center justify-center gap-2 px-4 text-center select-none">
                  <p className="text-xs text-muted-foreground">
                    {error ? `${error}` : ""}
                  </p>
                  <button
                    onClick={onRetry}
                    className="hidden rounded-full border border-white/20 bg-white/10 px-3 py-1 text-xs transition-colors hover:bg-white/20"
                  >
                    Retry
                  </button>
                </div>
              </CarouselItem>
            ) : (
              appPages.map((page, pageIndex) => (
                <CarouselItem key={pageIndex} className="pl-0">
                  <div
                    className={cn(
                      "grid w-full grid-cols-4 gap-2 px-4",
                      isDragging && "pointer-events-none"
                    )}
                  >
                    {page.map((app, appIndex) => (
                      <AppIcon key={appIndex} app={app} onSelect={onAppClick} />
                    ))}
                  </div>
                </CarouselItem>
              ))
            )}
          </CarouselContent>
        </Carousel>
      </div>

      <div className="flex w-full items-center justify-center">
        <div className="flex h-5.5 w-fit items-center justify-between gap-1 rounded-full border border-white/5 px-3.5">
          {appPages.map((_, index) => (
            <div
              key={index}
              onClick={() => carouselApi?.scrollTo(index)}
              className={cn(
                "aspect-square h-1.5 w-1.5 cursor-pointer rounded-full transition-colors duration-200",
                index === currentPage ? "bg-white" : "bg-white/40"
              )}
            />
          ))}
        </div>
      </div>
    </div>
  )
}

/** The iPhone mockup with its floating top bar and menu cards, as one unit */
export function PhoneStage({ className }: { className?: string }) {
  const [isMenuOpen, setIsMenuOpen] = useState(false)
  const [isAppsOpen, setIsAppsOpen] = useState(false)
  const [quality, setQuality] = useState(2)
  const [fps, setFPS] = useState(60)
  const [scale, setScale] = useState(0.6)
  const [apps, setApps] = useState<App[]>([])
  const [appsLoading, setAppsLoading] = useState(true)
  const [appsError, setAppsError] = useState<string | null>(null)
  const [isDevicesDrawerOpen, setIsDevicesDrawerOpen] = useState(false)
  const [isSessionsDrawerOpen, setIsSessionsDrawerOpen] = useState(false)

  // MongoDB data
  type DbDevice = {
    _id: string
    name: string
    model: string
    version: string
    status: string
    badge: "in_use" | "available" | "offline"
    in_use_by: string | null
    owned_by_current_user: boolean
    apps: string[]
    tailscale_ip?: string
    host_ports?: {
      wda?: number
      bridge?: number
      stream?: number
    }
  }
  type DbSession = {
    _id: string
    id: string
    devices: string[]
    started: string
    ended: string | null
    status: string
    end_reason: string | null
  }
  type DbUser = {
    _id: string
    name: string
    team: { _id: string; name: string } | null
    role: { _id: string; name: string } | null
  }

  const { data: dbDevicesRaw } = useLiveQuery<{ devices: DbDevice[] }>({
    url: "/api/devices",
    collection: "devices",
  })
  const { data: dbSessionsRaw } = useLiveQuery<{
    sessions: DbSession[]
    activeSession: DbSession | null
  }>({
    url: "/api/sessions",
    collection: "sessions",
  })
  const { data: dbUserRaw } = useLiveQuery<{ user: DbUser }>({
    url: "/api/users/me",
    collection: "users",
  })

  const dbDevices = dbDevicesRaw?.devices ?? []
  const dbSessions = dbSessionsRaw?.sessions ?? []
  const dbUser = dbUserRaw?.user ?? null
  const activeSession = dbSessionsRaw?.activeSession ?? null
  const pastSessions = dbSessions.filter((s) => s.status !== "active")

  // Preview selection for devices drawer (does NOT affect floating card)
  const [previewDeviceId, setPreviewDeviceId] = useState<string | null>(null)

  // Active device from DB session (what's actually in use)
  const sessionDeviceId = activeSession?.devices?.[0] ?? null
  const activeDevice = dbDevices.find((d) => d._id === sessionDeviceId) ?? null

  const isDeviceActive = useMemo(() => {
    return (
      !!activeSession &&
      activeSession.status === "active" &&
      !!activeDevice &&
      activeDevice.status === "online" &&
      !!activeDevice.host_ports &&
      typeof activeDevice.host_ports.stream === "number"
    )
  }, [activeSession, activeDevice])

  const activeEndpoints = useMemo(() => {
    const isLocal =
      typeof window !== "undefined" &&
      (window.location.hostname === "localhost" ||
        window.location.hostname === "127.0.0.1")

    const p = activeDevice?.host_ports || {}
    const streamPort = p.stream || 9200
    const wdaPort = p.wda || 8100
    const bridgePort = p.bridge || 9001

    if (isLocal) {
      return {
        streamBase: `http://localhost:${streamPort}`,
        streamWs: `ws://localhost:${streamPort}/stream.ws`,
        wdaBase: `http://localhost:${wdaPort}`,
        controlWs: `ws://localhost:${bridgePort}/ws`,
        controlHttp: `http://localhost:${bridgePort}`,
      }
    }

    // Remote via Nginx reverse proxy on meridianhub.cc (secure HTTPS/WSS, zero IP leak)
    const proto =
      typeof window !== "undefined" && window.location.protocol === "https:"
        ? "https:"
        : "http:"
    const wsProto = proto === "https:" ? "wss:" : "ws:"
    const host =
      typeof window !== "undefined" ? window.location.host : "meridianhub.cc"

    const targetIp = activeDevice?.tailscale_ip
    const prefix = targetIp ? `/dev/${targetIp}` : `/dev`

    return {
      streamBase: `${proto}//${host}${prefix}/${streamPort}`,
      streamWs: `${wsProto}//${host}${prefix}/${streamPort}/stream.ws`,
      wdaBase: `${proto}//${host}${prefix}/${wdaPort}`,
      controlWs: `${wsProto}//${host}${prefix}/${bridgePort}/ws`,
      controlHttp: `${proto}//${host}${prefix}/${bridgePort}`,
    }
  }, [activeDevice])

  // Preview device for drawer selection (falls back to first available)
  const previewDevice =
    dbDevices.find((d) => d._id === previewDeviceId) ?? dbDevices[0] ?? null

  // When drawer opens, set preview to session device or first available
  const wasDrawerOpen = useRef(false)
  useEffect(() => {
    const justOpened = isDevicesDrawerOpen && !wasDrawerOpen.current
    wasDrawerOpen.current = isDevicesDrawerOpen

    if (justOpened && dbDevices.length > 0) {
      if (sessionDeviceId) {
        setPreviewDeviceId(sessionDeviceId)
      } else {
        const first = dbDevices.find((d) => d.badge === "available")
        if (first) setPreviewDeviceId(first._id)
      }
    }
  }, [isDevicesDrawerOpen, sessionDeviceId, dbDevices])

  // Real-time elapsed timer for active session
  const [elapsed, setElapsed] = useState("0:00")
  useEffect(() => {
    if (!activeSession) {
      setElapsed("0:00")
      return
    }
    function tick() {
      const ms = Date.now() - new Date(activeSession!.started).getTime()
      const h = Math.floor(ms / 3600000)
      const m = Math.floor((ms % 3600000) / 60000)
      const s = Math.floor((ms % 60000) / 1000)
      setElapsed(
        h > 0
          ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
          : `${m}:${String(s).padStart(2, "0")}`
      )
    }
    tick()
    const iv = setInterval(tick, 1000)
    return () => clearInterval(iv)
  }, [activeSession])

  function formatDuration(started: string, ended: string | null): string {
    const ms =
      (ended ? new Date(ended) : new Date()).getTime() -
      new Date(started).getTime()
    const h = Math.floor(ms / 3600000)
    const m = Math.floor((ms % 3600000) / 60000)
    const s = Math.floor((ms % 60000) / 1000)
    if (h > 0)
      return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    return `${m}:${String(s).padStart(2, "0")}`
  }

  function formatSessionTime(started: string): string {
    const d = new Date(started)
    const now = new Date()
    const diffDays = Math.floor((now.getTime() - d.getTime()) / 86400000)
    if (diffDays === 0)
      return `Today, ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
    if (diffDays === 1) return "Yesterday"
    return d.toLocaleDateString([], { month: "short", day: "numeric" })
  }

  useEffect(() => {
    const handler = () => setIsDevicesDrawerOpen(true)
    if (typeof window !== "undefined") {
      window.addEventListener("open-devices-drawer", handler)
    }
    return () => {
      if (typeof window !== "undefined") {
        window.removeEventListener("open-devices-drawer", handler)
      }
    }
  }, [])

  useEffect(() => {
    const handler = () => setIsSessionsDrawerOpen(true)
    if (typeof window !== "undefined") {
      window.addEventListener("open-sessions-drawer", handler)
    }
    return () => {
      if (typeof window !== "undefined") {
        window.removeEventListener("open-sessions-drawer", handler)
      }
    }
  }, [])

  const loadApps = useCallback(async () => {
    setAppsLoading(true)
    setAppsError(null)
    try {
      const res = await fetch(`${activeEndpoints.controlHttp}/apps.json`)
      if (!res.ok) {
        const info = await res.json().catch(() => ({}))
        throw new Error(info.error ?? `bridge responded ${res.status}`)
      }
      const data = await res.json()
      setApps(
        (data.apps ?? []).map((a: RemoteApp) => ({
          name: a.name,
          iconUrl: `${activeEndpoints.controlHttp}/icon/${encodeURIComponent(a.bundleId)}.png`,
          state: "stopped" as const,
          bundleId: a.bundleId,
          pid: null,
        }))
      )
    } catch (e) {
      setAppsError(e instanceof Error ? e.message : String(e))
    } finally {
      setAppsLoading(false)
    }
  }, [activeEndpoints.controlHttp])

  useEffect(() => {
    loadApps()
  }, [loadApps])
  const [streamUrl, setStreamUrl] = useState("")
  const [streamStatus, setStreamStatus] = useState<StreamStatus>("connecting")
  const [streamStats, setStreamStats] = useState<StreamStats | null>(null)
  const controlWsRef = useRef<WebSocket | null>(null)
  const [controlReady, setControlReady] = useState(false)
  const [activeAction, setActiveAction] = useState<string | null>(null)
  const opsRef = useRef<
    Map<string, { state: "running" | "stopped"; ts: number }>
  >(new Map())
  const padRef = useRef<HTMLDivElement>(null)

  // Invisible pad over the stream: mirrors tools/hid_test.py's test page —
  // pointer events -> normalized {fx,fy} -> the bridge's gesture consumer.
  useEffect(() => {
    const el = padRef.current
    if (!el) return
    let down = false
    let startPoint = { fx: 0.5, fy: 0.5, time: 0 }
    let last = { fx: 0.5, fy: 0.5 }

    function frac(e: { clientX: number; clientY: number }): [number, number] {
      const r = el!.getBoundingClientRect()
      const fx = Math.min(Math.max((e.clientX - r.left) / r.width, 0), 1)
      const fy = Math.min(Math.max((e.clientY - r.top) / r.height, 0), 1)
      return [fx, fy]
    }
    function send(msg: Record<string, unknown>) {
      const ws = controlWsRef.current
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(msg))
      }
    }
    function handleDown(e: Event) {
      e.preventDefault()
      down = true
      const [fx, fy] = frac(e as PointerEvent)
      startPoint = { fx, fy, time: Date.now() }
      last = { fx, fy }
      send({ kind: "down", fx, fy })
    }
    function handleMove(e: Event) {
      if (!down) return
      const [fx, fy] = frac(e as PointerEvent)
      last = { fx, fy }
      send({ kind: "move", fx, fy })
    }
    function handleUp() {
      if (!down) return
      down = false
      send({ kind: "release", ...last })
    }

    el.addEventListener("pointerdown", handleDown)
    el.addEventListener("pointermove", handleMove)
    // Chromium-only, fires at full input rate — smoother drags than coalesced moves
    el.addEventListener("pointerrawupdate", handleMove)
    window.addEventListener("pointerup", handleUp)
    window.addEventListener("pointercancel", handleUp)
    window.addEventListener("blur", () => {
      if (down) {
        down = false
        send({ kind: "release", ...last })
      }
    })
    return () => {
      el.removeEventListener("pointerdown", handleDown)
      el.removeEventListener("pointermove", handleMove)
      el.removeEventListener("pointerrawupdate", handleMove)
      window.removeEventListener("pointerup", handleUp)
      window.removeEventListener("pointercancel", handleUp)
    }
  }, [activeSession])

  // Live running-state sync: while the Apps popover is open, poll the bridge.
  // Recent manual ops get a grace window so slow launches/kills don't flicker.
  useEffect(() => {
    if (!isAppsOpen) return
    let alive = true
    async function tick() {
      try {
        const res = await fetch(`${activeEndpoints.controlHttp}/apps/running.json`)
        if (!res.ok) throw new Error(String(res.status))
        const data = await res.json()
        const pidByBid = new Map<string, number>(
          ((data.running ?? []) as RunningApp[]).map((r) => [r.bundleId, r.pid])
        )
        if (!alive) return
        setApps((prev) =>
          prev.map((a) => {
            const bid = a.bundleId
            if (!bid) return a
            const pid = pidByBid.get(bid) ?? null
            const truth: AppState = pid != null ? "running" : "stopped"
            const op = opsRef.current.get(bid)
            const fresh = !!op && Date.now() - op.ts < 5000
            let st: AppState = truth
            if (fresh && op.state === "running" && truth === "stopped") {
              st = "running"
            } else if (fresh && op.state === "stopped" && truth === "running") {
              st = "closing"
            } else if (
              (op?.state === "running" && truth === "running") ||
              (op?.state === "stopped" && truth === "stopped")
            ) {
              opsRef.current.delete(bid)
            }
            return { ...a, state: st, pid }
          })
        )
      } catch {
        /* bridge hiccup — keep last known states */
      }
    }
    tick()
    const iv = setInterval(tick, 2500)
    return () => {
      alive = false
      clearInterval(iv)
    }
  }, [isAppsOpen])

  useEffect(() => {
    const url = buildStreamUrl(activeEndpoints.streamBase, quality, fps, scale)
    setStreamUrl(url)
    setStreamStatus("connecting")
    const timeout = setTimeout(() => {
      setStreamStatus((prev) => (prev === "connecting" ? "error" : prev))
    }, 3000)
    return () => clearTimeout(timeout)
  }, [activeEndpoints.streamBase, quality, fps, scale])

  // Control bridge (port 9001) — auto-reconnects while device is active;
  // cleanly terminates and closes when session stops.
  useEffect(() => {
    if (!isDeviceActive) {
      if (controlWsRef.current) {
        controlWsRef.current.close()
        controlWsRef.current = null
      }
      setControlReady(false)
      setIsAppsOpen(false)
      return
    }

    let closed = false
    let retry: ReturnType<typeof setTimeout>
    function connect() {
      if (closed || !isDeviceActive) return
      const ws = new WebSocket(activeEndpoints.controlWs)
      controlWsRef.current = ws
      ws.onopen = () => {
        if (!closed) setControlReady(true)
      }
      ws.onclose = () => {
        setControlReady(false)
        if (!closed && isDeviceActive) retry = setTimeout(connect, 500)
      }
      ws.onerror = () => ws.close()
    }
    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      controlWsRef.current?.close()
      controlWsRef.current = null
    }
  }, [activeEndpoints.controlWs, isDeviceActive])

  async function sendAction(name: string) {
    if (!isDeviceActive) return
    setActiveAction(name)
    setTimeout(() => setActiveAction((cur) => (cur === name ? null : cur)), 300)
    const ws = controlWsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ kind: "action", name }))
      return
    }

    try {
      if (name === "home") {
        await fetch(`${activeEndpoints.wdaBase}/wda/homescreen`, { method: "POST" })
      } else if (name === "lock") {
        await fetch(`${activeEndpoints.wdaBase}/wda/lock`, { method: "POST" })
      } else if (name === "volume-up") {
        const sid = await getWdaSession(activeEndpoints.wdaBase)
        if (sid) {
          await fetch(`${activeEndpoints.wdaBase}/session/${sid}/wda/pressButton`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ name: "volumeUp" }),
          })
        }
      } else if (name === "volume-down") {
        const sid = await getWdaSession(activeEndpoints.wdaBase)
        if (sid) {
          await fetch(`${activeEndpoints.wdaBase}/session/${sid}/wda/pressButton`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ name: "volumeDown" }),
          })
        }
      }
    } catch (e) {
      console.warn(`[Meridian] WDA action "${name}" failed:`, e)
    }
  }

  const [takingScreenshot, setTakingScreenshot] = useState(false)

  async function takeScreenshot() {
    if (takingScreenshot) return
    setTakingScreenshot(true)
    try {
      let dataUrl: string | null = null

      // 1. Try WDA full-res screenshot first
      try {
        const res = await fetch(`${activeEndpoints.wdaBase}/screenshot`)
        if (res.ok) {
          const json = await res.json()
          if (json?.value) {
            dataUrl = `data:image/png;base64,${json.value}`
          }
        }
      } catch {
        // ignore
      }

      // 2. Fallback to client-side GPU canvas capture
      if (!dataUrl) {
        const canvas = document.querySelector("canvas")
        if (canvas) {
          dataUrl = canvas.toDataURL("image/png")
        }
      }

      if (!dataUrl) {
        throw new Error("No screenshot buffer available")
      }

      const a = document.createElement("a")
      a.href = dataUrl
      a.download = `meridian-screenshot-${new Date().toISOString().replace(/[:.]/g, "-")}.png`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
    } catch (e) {
      console.warn("[Meridian] Screenshot failed:", e)
    } finally {
      setTimeout(() => setTakingScreenshot(false), 400)
    }
  }

  // Real-time typing: while armed, every keystroke is relayed to the phone.
  // Arming also mounts the virtual keyboard (hiding the iPhone's on-screen
  // one); disarming unmounts it again so the software keyboard returns.
  const [typingMode, setTypingMode] = useState(false)

  function setTypingModeWithKeyboard(on: boolean) {
    setTypingMode(on)
    const ws = controlWsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ kind: "keyboard", on }))
    }
  }

  useEffect(() => {
    ;(window as unknown as { __iusTyping: boolean }).__iusTyping = typingMode
    if (!typingMode) return

    function sendKey(msg: Record<string, unknown>, keyStr?: string) {
      const ws = controlWsRef.current
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(msg))
        return
      }
      if (keyStr) {
        getWdaSession(activeEndpoints.wdaBase).then((sid) => {
          if (sid) {
            fetch(`${activeEndpoints.wdaBase}/session/${sid}/wda/keys`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ value: [keyStr] }),
            }).catch(() => {})
          }
        })
      }
    }

    // Zero-latency mode: relay the PHYSICAL key (KeyboardEvent.code) as a raw
    // scancode — the iPhone decodes it with its own configured layout, so any
    // distribution and dead keys behave exactly like a real keyboard.
    const MOD_CODES = new Set([
      "ShiftLeft",
      "ShiftRight",
      "ControlLeft",
      "ControlRight",
      "AltLeft",
      "AltRight",
      "MetaLeft",
      "MetaRight",
    ])

    function onKeyDown(e: KeyboardEvent) {
      if (MOD_CODES.has(e.code)) return
      if (e.key === "Escape") {
        setTypingModeWithKeyboard(false)
        return
      }
      if (e.key === "Dead") return // Dead key composing accent; wait for composed char

      // Let browser shortcuts pass through unless AltGr (e.g. Cmd+R, Ctrl+T, Ctrl+C)
      const altgr = e.ctrlKey && e.altKey
      if ((e.metaKey || (e.ctrlKey && !altgr)) && e.key !== "v" && e.key !== "V") {
        return
      }

      e.preventDefault()

      let shift = e.shiftKey
      if (e.code.startsWith("Key") && e.getModifierState("CapsLock")) {
        shift = !shift
      }

      let textToSend: string | null = null
      if (e.key === "Backspace") {
        textToSend = "\b"
      } else if (e.key === "Enter") {
        textToSend = "\n"
      } else if (e.key === "Tab") {
        textToSend = "\t"
      } else if (e.key.length === 1) {
        textToSend = e.key // Exact character from user layout (ñ, á, @, €, ¿, digits, symbols)
      }

      const isSpecial = e.key.length === 1 && (e.key.charCodeAt(0) > 127 || "¿¡€£¥§".includes(e.key))

      sendKey({
        kind: "key_down",
        text: textToSend,
        key: e.key,
        code: e.code,
        isSpecial,
        shift,
        ctrl: e.ctrlKey,
        alt: e.altKey,
        meta: e.metaKey,
      }, textToSend || e.key)
    }

    function onKeyUp(e: KeyboardEvent) {
      if (MOD_CODES.has(e.code)) return
      if (e.key === "Escape") return
      sendKey({
        kind: "key_up",
        code: e.code,
        key: e.key,
        shift: e.shiftKey,
        ctrl: e.ctrlKey,
        alt: e.altKey,
        meta: e.metaKey,
      })
    }

    function onPaste(e: ClipboardEvent) {
      const text = e.clipboardData?.getData("text")
      if (!text) return
      e.preventDefault()
      const ws = controlWsRef.current
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ kind: "paste", text }))
        return
      }
      getWdaSession(activeEndpoints.wdaBase).then((sid) => {
        if (sid) {
          fetch(`${activeEndpoints.wdaBase}/session/${sid}/wda/keys`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ value: [text] }),
          }).catch(() => {})
        }
      })
    }

    window.addEventListener("keydown", onKeyDown, true)
    window.addEventListener("keyup", onKeyUp, true)
    window.addEventListener("paste", onPaste, true)
    return () => {
      window.removeEventListener("keydown", onKeyDown, true)
      window.removeEventListener("keyup", onKeyUp, true)
      window.removeEventListener("paste", onPaste, true)
    }
  }, [typingMode])

  function handleAppClick(app: App) {
    const bid = app.bundleId
    if (!bid) return
    if (app.state === "running" && app.pid) {
      // stop for real: SIGKILL via bridge; poll confirms "stopped"
      opsRef.current.set(bid, { state: "stopped", ts: Date.now() })
      setApps((prev) =>
        prev.map((a) => (a.bundleId === bid ? { ...a, state: "closing" } : a))
      )
      const pid = app.pid
      fetch(`${activeEndpoints.controlHttp}/app/kill/${pid}`, { method: "POST" }).catch(() =>
        opsRef.current.delete(bid)
      )
    } else if (app.state === "stopped") {
      // launch via bridge; poll confirms "running" (grace period avoids flicker)
      opsRef.current.set(bid, { state: "running", ts: Date.now() })
      setApps((prev) =>
        prev.map((a) =>
          a.bundleId === bid ? { ...a, state: "running", pid: -1 } : a
        )
      )
      fetch(`${activeEndpoints.controlHttp}/app/launch/${encodeURIComponent(bid)}`, {
        method: "POST",
      }).catch(() => opsRef.current.delete(bid))
    }
  }

  function handleAppsOpenChange(
    open: boolean,
    details?: { reason?: string; event?: Event }
  ) {
    const target = details?.event?.target as Element | null
    const info = {
      open,
      reason: details?.reason ?? null,
      eventType: details?.event?.type ?? null,
      targetSlot: target?.getAttribute?.("data-slot") ?? null,
      targetTag: target?.tagName ?? null,
      isInPopup: !!target?.closest?.("[data-slot='popover-content']"),
    }
    console.log("[apps-popover]", JSON.stringify(info))
    ;(window as unknown as { __lastAppsClose: unknown }).__lastAppsClose = info
    if (!open && details?.reason === "outside-press" && target) {
      if (target.closest("[data-slot='popover-content']")) return
    }
    setIsAppsOpen(open)
  }

  return (
    <div
      className={cn("flex h-full items-center justify-center pt-18", className)}
    >
      <AnimatePresence mode="wait">
        {/* Empty state — shown when no active session */}
        {!isDeviceActive && (
          <motion.div
            key="empty"
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ duration: 0.3, ease: "easeOut" }}
          >
            <Empty className="-mt-18 max-w-xs border-white/5">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <HugeiconsIcon
                    icon={SmartPhoneIcon}
                    size={20}
                    strokeWidth={1.5}
                    className="opacity-70"
                  />
                </EmptyMedia>
                <EmptyTitle className="text-base">No active session</EmptyTitle>
                <EmptyDescription>
                  Start a new session, select a device and begin remote control.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button
                  className="w-fit px-4"
                  onClick={() => {
                    if (typeof window !== "undefined") {
                      window.dispatchEvent(new Event("open-devices-drawer"))
                    }
                  }}
                >
                  Start New Session
                </Button>
              </EmptyContent>
            </Empty>
          </motion.div>
        )}
        {/* Phone + floating cards — shown when session is active */}
        {isDeviceActive && (
          <motion.div
            key="phone"
            initial={{ opacity: 0, scale: 0.95, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 10 }}
            transition={{ duration: 0.35, ease: "easeOut" }}
            className="relative aspect-[1748/3532] h-full max-h-full cursor-pointer"
          >
            <div className="absolute inset-0 drop-shadow-xl drop-shadow-primary/20">
              {/* The screen area: Ultra-low latency H.264 WebCodecs GPU stream with MSE / MJPEG fallback */}
              <div className="absolute top-[2.2%] right-[5.55%] bottom-[2.30%] left-[5.45%] z-0 overflow-hidden rounded-[1.5rem]">
                <H264StreamPlayer
                  wsUrl={activeEndpoints.streamWs}
                  fallbackUrl={streamUrl}
                  scale={scale}
                  fps={fps}
                  bitrateMbps={2.5}
                  onStatusChange={setStreamStatus}
                  onStatsUpdate={setStreamStats}
                />
                {/* Invisible gesture pad — streams normalized coords to the HID
              bridge or directly to WDA */}
                <div
                  ref={padRef}
                  aria-hidden
                  className="absolute inset-0 z-20 cursor-pointer touch-none select-none"
                />
              </div>
              {/* The iPhone frame over the screen */}
              <Image
                src="/iPhone13.png"
                alt="iPhone 14 Mockup"
                fill
                className="pointer-events-none z-10 object-contain"
              />
            </div>
            {/* Floating top bar card */}
            <div className="absolute -top-[4.5rem] left-0 z-0 flex h-14 w-full items-center justify-between rounded-[32px] border border-white/5 bg-secondary pr-3 pl-6 shadow-xl shadow-primary/5">
              <div className="flex flex-col gap-0">
                <span className="truncate text-xs font-medium">
                  {activeDevice?.name ?? dbUser?.team?.name ?? "No device"}
                </span>
                <span className="truncate text-[0.6875rem] text-muted-foreground">
                  {activeDevice
                    ? `${activeDevice.model} \u2022 ${activeDevice.version}`
                    : "Select a device"}
                </span>
              </div>
              <div className="flex gap-1.5 rounded-full px-2 py-2">
                <Tooltip>
                  <TooltipTrigger>
                    <HugeiconsIcon
                      icon={SmartPhoneIcon}
                      size={16}
                      strokeWidth={2}
                      className="cursor-pointer opacity-70 transition-transform duration-300 hover:opacity-100"
                      onClick={() => setIsDevicesDrawerOpen(true)}
                    />
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>Devices</p>
                  </TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger>
                    <HugeiconsIcon
                      icon={CircleChevronDownIcon}
                      size={16}
                      strokeWidth={2}
                      className={`cursor-pointer opacity-70 transition-transform duration-300 hover:opacity-100 ${isMenuOpen ? "rotate-90" : "-rotate-90"}`}
                      onClick={() => setIsMenuOpen(!isMenuOpen)}
                    />
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>Menu</p>
                  </TooltipContent>
                </Tooltip>
              </div>
            </div>
            {/* Floating menu card */}
            <AnimatePresence>
              {isMenuOpen && (
                <motion.div
                  initial={{ opacity: 0, x: -10, scale: 0.95 }}
                  animate={{ opacity: 1, x: 0, scale: 1 }}
                  exit={{ opacity: 0, x: -10, scale: 0.95 }}
                  transition={{ duration: 0.2, ease: "easeOut" }}
                  className="absolute -top-[4.5rem] left-[105%] z-0 flex h-fit w-12 items-start justify-center rounded-[32px] border border-white/5 bg-secondary py-3 shadow-xl shadow-primary/5"
                >
                  <div className="flex flex-col gap-2 rounded-full px-1 py-2">
                    <Tooltip>
                      <TooltipTrigger>
                        <HugeiconsIcon
                          icon={Home03Icon}
                          size={16}
                          strokeWidth={2}
                          onClick={() => sendAction("home")}
                          className={cn(
                            "cursor-pointer transition-all",
                            activeAction === "home"
                              ? "scale-90 opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Home</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger render={<span className="flex" />}>
                        <Popover
                          open={isAppsOpen}
                          onOpenChange={handleAppsOpenChange}
                        >
                          <PopoverTrigger>
                            <HugeiconsIcon
                              icon={AppStoreIcon}
                              size={16}
                              strokeWidth={2}
                              className="cursor-pointer opacity-60 hover:opacity-100"
                            />
                          </PopoverTrigger>
                          <PopoverContent
                            align="start"
                            side="right"
                            sideOffset={25}
                            className="-mt-11 flex h-56 w-fit bg-secondary px-0 py-2"
                          >
                            <AppsCarousel
                              apps={apps}
                              onAppClick={handleAppClick}
                              isLoading={appsLoading}
                              error={appsError}
                              onRetry={loadApps}
                            />
                          </PopoverContent>
                        </Popover>
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Apps</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger>
                        <HugeiconsIcon
                          icon={VolumeHighIcon}
                          size={16}
                          strokeWidth={2}
                          onClick={() => sendAction("volume-up")}
                          className={cn(
                            "cursor-pointer transition-all",
                            activeAction === "volume-up"
                              ? "scale-90 opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Volume Up</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger>
                        <HugeiconsIcon
                          icon={VolumeLowIcon}
                          size={16}
                          strokeWidth={2}
                          onClick={() => sendAction("volume-down")}
                          className={cn(
                            "cursor-pointer transition-all",
                            activeAction === "volume-down"
                              ? "scale-90 opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Volume Down</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger className="hidden">
                        <HugeiconsIcon
                          icon={LockPasswordIcon}
                          size={16}
                          strokeWidth={2}
                          onClick={() => sendAction("lock")}
                          className={cn(
                            "cursor-pointer transition-all",
                            activeAction === "lock"
                              ? "scale-90 opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Lock</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger>
                        <HugeiconsIcon
                          icon={FocusIcon}
                          size={16}
                          strokeWidth={2}
                          onClick={takeScreenshot}
                          className={cn(
                            "cursor-pointer transition-all",
                            takingScreenshot
                              ? "scale-90 text-primary opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>{takingScreenshot ? "Capturing..." : "Screenshot"}</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger
                        className="cursor-pointer focus:outline-none focus-visible:outline-none focus:ring-0 focus-visible:ring-0"
                        onClick={(e) => {
                          e.preventDefault()
                          setTypingModeWithKeyboard(!typingMode)
                        }}
                      >
                        <HugeiconsIcon
                          icon={KeyboardIcon}
                          size={16}
                          strokeWidth={2}
                          className={cn(
                            "cursor-pointer transition-all",
                            typingMode
                              ? "text-primary opacity-100"
                              : "opacity-60 hover:opacity-100"
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>
                          {typingMode
                            ? "Typing ON — Esc to stop"
                            : "Typing Mode"}
                        </p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger render={<span className="flex" />}>
                        <Popover>
                          <PopoverTrigger>
                            <HugeiconsIcon
                              icon={Settings01Icon}
                              size={16}
                              strokeWidth={2}
                              className="cursor-pointer opacity-60 hover:opacity-100"
                            />
                          </PopoverTrigger>
                          <PopoverContent
                            align="center"
                            side="right"
                            sideOffset={25}
                            className="w-fit bg-secondary px-4 py-2"
                          >
                            <div className="flex w-34 flex-col gap-3.5 pt-1 pb-2">
                              <div className="flex flex-col gap-2">
                                <div className="flex items-center justify-between select-none">
                                  <span className="text-xs font-medium">
                                    Quality
                                  </span>
                                  <span className="text-xs text-muted-foreground">
                                    {quality === 3
                                      ? "High"
                                      : quality === 2
                                        ? "Good"
                                        : "Stable"}
                                  </span>
                                </div>
                                <Slider
                                  value={quality}
                                  onValueChange={(value) =>
                                    setQuality(value as number)
                                  }
                                  min={1}
                                  max={3}
                                  step={1}
                                />
                              </div>
                              <div className="flex flex-col gap-2">
                                <div className="flex items-center justify-between select-none">
                                  <span className="text-xs font-medium">
                                    Resolution
                                  </span>
                                  <span className="text-xs text-muted-foreground">
                                    {getResolutionLabel(scale)}
                                  </span>
                                </div>
                                <Slider
                                  value={scale}
                                  onValueChange={(value) =>
                                    setScale(value as number)
                                  }
                                  min={0.5}
                                  max={1.5}
                                  step={0.25}
                                />
                              </div>
                              <div className="flex hidden flex-col gap-2">
                                <div className="flex items-center justify-between select-none">
                                  <span className="text-xs font-medium">
                                    Performance
                                  </span>
                                  <span className="text-xs text-muted-foreground">
                                    {fps} FPS
                                  </span>
                                </div>
                                <Slider
                                  value={fps}
                                  onValueChange={(value) =>
                                    setFPS(value as number)
                                  }
                                  min={30}
                                  max={60}
                                  step={5}
                                />
                              </div>
                            </div>
                          </PopoverContent>
                        </Popover>
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Settings</p>
                      </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger>
                        <HugeiconsIcon
                          icon={CircleXIcon}
                          size={16}
                          strokeWidth={2}
                          className="cursor-pointer text-destructive opacity-60 hover:opacity-100"
                        />
                      </TooltipTrigger>
                      <TooltipContent side="right" className="select-none">
                        <p>Disconnect</p>
                      </TooltipContent>
                    </Tooltip>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
      {/* Drawers — placed outside AnimatePresence so they render always */}
      <Drawer
        open={isDevicesDrawerOpen}
        onOpenChange={setIsDevicesDrawerOpen}
        swipeDirection="right"
      >
        <DrawerContent className="w-80 border-l border-white/5 bg-secondary">
          <DrawerHeader className="">
            <DrawerTitle className="text-sm">Devices</DrawerTitle>
            <DrawerDescription className="-mt-1 text-xs">
              Choose a device to use.
            </DrawerDescription>
          </DrawerHeader>
          <div className="relative flex-1 overflow-hidden [mask-image:linear-gradient(to_bottom,black_calc(100%-3rem),transparent_100%)]">
            <div className="h-full overflow-y-auto p-4 pt-3 pb-12">
              <RadioGroup
                value={previewDeviceId ?? ""}
                onValueChange={setPreviewDeviceId}
                className="flex flex-col gap-2"
              >
                {dbDevices.map((d) => (
                  <div key={d._id}>
                    <RadioGroupItem
                      value={d._id}
                      id={`device-${d._id}`}
                      className="peer sr-only absolute"
                    />
                    <Label
                      htmlFor={`device-${d._id}`}
                      className={cn(
                        "flex items-center justify-start rounded-2xl border border-white/5 bg-white/5 py-2.5 pr-2 pl-3 transition-colors peer-data-checked:border-primary peer-data-checked:bg-primary/20 hover:bg-white/10",
                        d.badge === "available" || (d.badge === "in_use" && d.owned_by_current_user)
                          ? "cursor-pointer"
                          : "pointer-events-none opacity-55"
                      )}
                    >
                      <HugeiconsIcon
                        icon={SmartPhoneIcon}
                        size={24}
                        strokeWidth={1.25}
                      />
                      <div className="flex flex-col items-start gap-0.5">
                        <span className="flex flex-row items-center gap-1.5 text-left text-xs font-medium">
                          {d.name}
                          <Badge
                            variant="outline"
                            className="absolute right-6.5 h-4 py-0 text-[10px] leading-none"
                          >
                            {d.badge === "in_use"
                              ? "In Use"
                              : d.badge === "available"
                                ? "Available"
                                : "Offline"}
                          </Badge>
                        </span>
                        <span className="text-left text-[0.6875rem] text-muted-foreground">
                          {d.model}
                          <span className="mx-0.75">•</span>
                          {d.version}
                        </span>
                      </div>
                    </Label>
                  </div>
                ))}
                {dbDevices.length === 0 && (
                  <p className="py-4 text-center text-xs text-muted-foreground">
                    No devices found.
                  </p>
                )}
              </RadioGroup>
            </div>
          </div>
          <DrawerFooter className="mt-auto pb-5">
            <Button
              className="w-full"
              disabled={
                !previewDevice ||
                previewDevice.badge === "offline" ||
                (previewDevice.badge === "in_use" &&
                  !previewDevice.owned_by_current_user) ||
                (!!activeSession && previewDeviceId === sessionDeviceId)
              }
              onClick={async () => {
                if (!previewDevice) return
                if (activeSession) {
                  if (previewDeviceId === sessionDeviceId) return
                  const res = await fetch(
                    `/api/sessions/${activeSession._id}`,
                    {
                      method: "PATCH",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ swap_device: previewDevice._id }),
                    }
                  )
                  if (res.ok) {
                    setIsDevicesDrawerOpen(false)
                  }
                } else {
                  const res = await fetch("/api/sessions", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ deviceId: previewDevice._id }),
                  })
                  if (res.ok) {
                    setIsDevicesDrawerOpen(false)
                  }
                }
              }}
            >
              Use Device
            </Button>
          </DrawerFooter>
        </DrawerContent>
      </Drawer>
      <Drawer
        open={isSessionsDrawerOpen}
        onOpenChange={setIsSessionsDrawerOpen}
        swipeDirection="right"
      >
        <DrawerContent className="w-80 border-l border-white/5 bg-secondary">
          <DrawerHeader>
            <DrawerTitle className="text-sm">Sessions</DrawerTitle>
            <DrawerDescription className="-mt-1 text-xs">
              Your active and recent sessions.
            </DrawerDescription>
          </DrawerHeader>
          <div className="relative flex-1 overflow-hidden [mask-image:linear-gradient(to_bottom,black_calc(100%-3rem),transparent_100%)]">
            <div className="h-full overflow-y-auto p-4 pt-3 pb-12">
              <div className="flex flex-col gap-2">
                {activeSession && (
                  <div className="flex cursor-pointer items-center gap-2.5 rounded-2xl border border-primary/30 bg-primary/10 py-2.5 pr-2 pl-3 transition-colors hover:bg-primary/15">
                    <HugeiconsIcon
                      icon={Activity01Icon}
                      size={22}
                      strokeWidth={1.25}
                      className=""
                    />
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-xs font-medium select-none">
                        Current Session
                        <span className="absolute right-7 font-mono text-[11px] tracking-tight text-muted-foreground select-none">
                          {elapsed}
                        </span>
                      </span>
                      <span className="truncate text-[0.6875rem] text-muted-foreground select-none">
                        {activeSession.devices.length} Device
                        {activeSession.devices.length !== 1 ? "s" : ""}
                        <span className="mx-0.75">•</span>
                        {activeSession.devices
                          .map(
                            (did) => dbDevices.find((d) => d._id === did)?.name
                          )
                          .filter(Boolean)
                          .join(", ") || "Unknown device"}
                      </span>
                    </div>
                  </div>
                )}
                {pastSessions.map((s) => (
                  <div
                    key={s._id}
                    className="flex cursor-pointer items-center gap-2.5 rounded-2xl border border-white/5 bg-white/5 py-2.5 pr-3 pl-3 transition-colors hover:bg-white/10"
                  >
                    <HugeiconsIcon
                      icon={Activity01Icon}
                      size={22}
                      strokeWidth={1.25}
                      className=""
                    />
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-xs font-medium select-none">
                        {s.id}
                        <span className="absolute right-7 font-mono text-[11px] tracking-tight text-muted-foreground select-none">
                          {formatSessionTime(s.started)}
                        </span>
                      </span>
                      <span className="truncate text-[0.6875rem] text-muted-foreground select-none">
                        {s.devices.length} Device
                        {s.devices.length !== 1 ? "s" : ""}
                        <span className="mx-0.75">•</span>
                        {s.devices
                          .map(
                            (did) => dbDevices.find((d) => d._id === did)?.name
                          )
                          .filter(Boolean)
                          .join(", ") || "Unknown device"}
                      </span>
                    </div>
                  </div>
                ))}
                {dbSessions.length === 0 && (
                  <p className="py-4 text-center text-xs text-muted-foreground">
                    No sessions found.
                  </p>
                )}
              </div>
            </div>
          </div>
          <DrawerFooter className="mt-auto pb-5">
            {activeSession ? (
              <Button
                variant="destructive"
                onClick={async () => {
                  const res = await fetch(
                    `/api/sessions/${activeSession._id}`,
                    {
                      method: "PATCH",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ end_reason: "user" }),
                    }
                  )
                  if (res.ok) {
                    setIsSessionsDrawerOpen(false)
                  }
                }}
              >
                End Current Session
              </Button>
            ) : (
              <Button
                variant="default"
                onClick={() => {
                  setIsSessionsDrawerOpen(false)
                  setIsDevicesDrawerOpen(true)
                }}
              >
                Start New Session
              </Button>
            )}
          </DrawerFooter>
        </DrawerContent>
      </Drawer>
    </div>
  )
}
