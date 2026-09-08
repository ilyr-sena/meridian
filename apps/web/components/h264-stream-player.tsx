"use client"

import { memo, useEffect, useRef, useState } from "react"
import { cn } from "@workspace/ui/lib/utils"

export interface StreamStats {
  fps: number
  mbps: number
  latencyMs: number
  codec: string
}

export interface H264StreamPlayerProps {
  wsUrl?: string
  fallbackUrl?: string
  className?: string
  style?: React.CSSProperties
  scale?: number
  fps?: number
  bitrateMbps?: number
  onStatusChange?: (status: "connecting" | "loaded" | "error") => void
  onStatsUpdate?: (stats: StreamStats) => void
}

export const H264StreamPlayer = memo(function H264StreamPlayer({
  wsUrl = "ws://localhost:9200/stream.ws",
  fallbackUrl = "http://localhost:9200/stream",
  className,
  style,
  scale = 0.6,
  fps = 60,
  bitrateMbps = 2.5,
  onStatusChange,
  onStatsUpdate,
}: H264StreamPlayerProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const [renderMode, setRenderMode] = useState<"webcodecs" | "mse" | "mjpeg">("webcodecs")
  const [streamStatus, setStreamStatus] = useState<"connecting" | "loaded" | "error">("connecting")

  const stateRef = useRef({
    ws: null as WebSocket | null,
    decoder: null as any,
    mediaSource: null as MediaSource | null,
    sourceBuffer: null as SourceBuffer | null,
    pendingBuffers: [] as Uint8Array[],
    isAppending: false,
    rxBytes: 0,
    rxStamp: performance.now(),
    fpsCount: 0,
    fpsStamp: performance.now(),
    latencyMs: 0,
    codecStr: "h264",
    unmounted: false,
    hasSeenKeyFrame: false,
    animFrameId: null as number | null,
  })

  useEffect(() => {
    onStatusChange?.(streamStatus)
  }, [streamStatus, onStatusChange])

  useEffect(() => {
    const s = stateRef.current
    s.unmounted = false

    const hasWebCodecs =
      typeof window !== "undefined" && typeof (window as any).VideoDecoder === "function"

    // Stats ticker (every 800ms)
    const statsTimer = setInterval(() => {
      const now = performance.now()
      const rxDt = Math.max(0.001, (now - s.rxStamp) / 1000)
      s.rxStamp = now
      const mbps = Number(((s.rxBytes * 8) / rxDt / 1e6).toFixed(2))
      s.rxBytes = 0

      const fpsDt = Math.max(0.001, (now - s.fpsStamp) / 1000)
      s.fpsStamp = now
      const fps = Math.round(s.fpsCount / fpsDt)
      s.fpsCount = 0

      onStatsUpdate?.({
        fps,
        mbps,
        latencyMs: s.latencyMs,
        codec: s.codecStr,
      })
    }, 800)

    // Latency RTT ping (every 1000ms)
    const pingTimer = setInterval(() => {
      if (s.ws && s.ws.readyState === WebSocket.OPEN) {
        try {
          s.ws.send(JSON.stringify({ kind: "ping", t: performance.now() }))
        } catch {
          // ignore
        }
      }
    }, 1000)

    // MSE drift controller
    const mseTimer = setInterval(() => {
      const v = videoRef.current
      const sb = s.sourceBuffer
      if (v && sb && sb.buffered.length > 0 && !s.decoder) {
        const end = sb.buffered.end(sb.buffered.length - 1)
        const behind = end - v.currentTime
        if (behind > 1.0) {
          v.currentTime = end - 0.05
          v.playbackRate = 1.0
        } else if (behind > 0.12) {
          v.playbackRate = 1.08
        } else {
          v.playbackRate = 1.0
        }
        if (v.paused || v.ended) {
          v.play().catch(() => {})
        }
        if (behind > 6 && !sb.updating && !s.isAppending) {
          try {
            sb.remove(sb.buffered.start(0), end - 2.0)
          } catch {
            // ignore
          }
        }
      }
    }, 200)

    function drainMSE() {
      const sb = s.sourceBuffer
      if (!sb || sb.updating || s.isAppending || s.pendingBuffers.length === 0) return
      const next = s.pendingBuffers.shift()
      if (!next) return
      s.isAppending = true
      try {
        sb.appendBuffer(next.buffer as ArrayBuffer)
      } catch {
        // buffer full / error
      }
      s.isAppending = false
    }

    function initMSE(mime: string) {
      if (s.mediaSource) return
      const ms = new MediaSource()
      s.mediaSource = ms
      let initialSeekDone = false

      ms.addEventListener("sourceopen", () => {
        try {
          const sb = ms.addSourceBuffer(mime)
          s.sourceBuffer = sb
          sb.mode = "segments"
          sb.addEventListener("updateend", () => {
            drainMSE()
            if (!initialSeekDone && sb.buffered.length > 0) {
              const end = sb.buffered.end(sb.buffered.length - 1)
              if (end > 0.05 && videoRef.current) {
                videoRef.current.currentTime = end
                initialSeekDone = true
              }
            }
          })
          drainMSE()
        } catch (e) {
          console.warn("[Meridian] MSE setup failed, falling back to MJPEG:", e)
          setRenderMode("mjpeg")
        }
      })

      if (videoRef.current) {
        videoRef.current.src = URL.createObjectURL(ms)
        videoRef.current.play().catch(() => {})
      }
    }

    function initWebCodecs(m: { codec: string; avcC?: string }) {
      const canvas = canvasRef.current
      if (!canvas || !hasWebCodecs) {
        initMSE(`video/mp4; codecs="${m.codec}"`)
        setRenderMode("mse")
        return
      }

      if (s.decoder) {
        try {
          s.decoder.close()
        } catch {
          // ignore
        }
        s.decoder = null
      }

      const ctx2d = canvas.getContext("2d", { alpha: false, desynchronized: true })
      if (!ctx2d) {
        initMSE(`video/mp4; codecs="${m.codec}"`)
        setRenderMode("mse")
        return
      }
      ctx2d.imageSmoothingEnabled = true
      ctx2d.imageSmoothingQuality = "high"

      const rawCodec = m.codec || "avc1.64002a"
      const codec = rawCodec.toLowerCase()

      // Parse and clean AVCDecoderConfigurationRecord
      let desc: Uint8Array | undefined
      if (m.avcC) {
        try {
          const rawAvcC = atob(m.avcC)
          const rawBytes = new Uint8Array(rawAvcC.length)
          for (let i = 0; i < rawAvcC.length; i++) {
            rawBytes[i] = rawAvcC.charCodeAt(i)
          }

          // Strip 8-byte box header if present (4 bytes size + "avcC")
          if (
            rawBytes.length > 8 &&
            rawBytes[4] === 0x61 &&
            rawBytes[5] === 0x76 &&
            rawBytes[6] === 0x63 &&
            rawBytes[7] === 0x43
          ) {
            desc = rawBytes.subarray(8)
          } else if (rawBytes[0] === 1) {
            desc = rawBytes
          } else {
            // Find 0x01 configurationVersion
            const idx = rawBytes.indexOf(1)
            if (idx !== -1 && idx + 6 < rawBytes.length) {
              desc = rawBytes.subarray(idx)
            } else {
              desc = rawBytes
            }
          }
        } catch (e) {
          console.warn("[Meridian] Could not parse avcC:", e)
        }
      }

      let pendingFrame: any = null

      function renderLoop() {
        if (s.unmounted) return
        const cvs = canvasRef.current
        if (pendingFrame && cvs && ctx2d) {
          const f = pendingFrame
          pendingFrame = null
          if (cvs.width !== f.displayWidth || cvs.height !== f.displayHeight) {
            cvs.width = f.displayWidth
            cvs.height = f.displayHeight
          }
          ctx2d.drawImage(f, 0, 0)
          f.close()
          s.fpsCount++
          setStreamStatus("loaded")
        }
        s.animFrameId = requestAnimationFrame(renderLoop)
      }

      if (s.animFrameId) {
        cancelAnimationFrame(s.animFrameId)
      }
      s.animFrameId = requestAnimationFrame(renderLoop)

      const VideoDecoderClass = (window as any).VideoDecoder
      const decoder = new VideoDecoderClass({
        output: (frame: any) => {
          if (pendingFrame) {
            pendingFrame.close()
          }
          pendingFrame = frame
        },
        error: (err: any) => {
          console.warn("[Meridian] WebCodecs decode warning (will resync on next IDR):", err)
          s.hasSeenKeyFrame = false
        },
      })

      // Multi-tier configuration: prefer hardware & low-latency, with graceful fallback
      let configured = false

      if (desc && desc.length > 0) {
        try {
          decoder.configure({
            codec,
            description: desc,
            hardwareAcceleration: "prefer-hardware",
            optimizeForLatency: true,
          })
          configured = true
        } catch {
          try {
            decoder.configure({
              codec,
              description: desc,
              hardwareAcceleration: "prefer-hardware",
            })
            configured = true
          } catch {
            try {
              decoder.configure({ codec, description: desc })
              configured = true
            } catch {
              console.warn("[Meridian] Config with description failed; attempting in-band config...")
            }
          }
        }
      }

      if (!configured) {
        try {
          decoder.configure({
            codec,
            hardwareAcceleration: "prefer-hardware",
            optimizeForLatency: true,
          })
          configured = true
        } catch {
          try {
            decoder.configure({ codec })
            configured = true
          } catch (e) {
            console.error("[Meridian] All WebCodecs configurations failed:", e)
          }
        }
      }

      if (configured) {
        s.decoder = decoder
        s.hasSeenKeyFrame = false
        s.codecStr = `${codec} (gpu)`
        setRenderMode("webcodecs")
      } else {
        initMSE(`video/mp4; codecs="${codec}"`)
        setRenderMode("mse")
      }
    }

    let reconnectTimer: ReturnType<typeof setTimeout>

    function connect() {
      if (s.unmounted) return
      setStreamStatus("connecting")

      let ws: WebSocket
      try {
        ws = new WebSocket(wsUrl)
      } catch (e) {
        console.warn("[Meridian] WebSocket connection error:", e)
        setStreamStatus("error")
        reconnectTimer = setTimeout(connect, 1500)
        return
      }

      s.ws = ws
      ws.binaryType = "arraybuffer"

      ws.onopen = () => {
        if (s.unmounted) return
        setStreamStatus("loaded")
        // Request 720p 60fps low-latency stream tuning from runner
        try {
          ws.send(
            JSON.stringify({
              op: "tune",
              scale: scale,
              bitrateMbps: bitrateMbps,
              maxFps: fps,
              keyframeSeconds: 1.0,
            })
          )
        } catch {
          // ignore
        }
      }

      ws.onclose = () => {
        if (s.unmounted) return
        setStreamStatus("error")
        reconnectTimer = setTimeout(connect, 1000)
      }

      ws.onerror = () => {
        ws.close()
      }

      ws.onmessage = (ev) => {
        if (s.unmounted) return
        if (typeof ev.data === "string") {
          let m: any
          try {
            m = JSON.parse(ev.data)
          } catch {
            return
          }
          if (m.codec) {
            if (hasWebCodecs && m.avcC) {
              initWebCodecs(m)
            } else {
              initMSE(`video/mp4; codecs="${m.codec}"`)
              setRenderMode("mse")
            }
          }
          if (m.t) {
            s.latencyMs = Math.round(performance.now() - m.t)
          }
        } else {
          const data = new Uint8Array(ev.data)
          s.rxBytes += data.byteLength

          if (s.decoder && s.decoder.state === "configured") {
            if (data.length < 8) return
            const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
            const b0 = data[4] ?? 0
            const b1 = data[5] ?? 0
            const b2 = data[6] ?? 0
            const b3 = data[7] ?? 0
            const tag = String.fromCharCode(b0, b1, b2, b3)
            if (tag === "ftyp") return
            if (tag === "moof") {
              const moofLen = view.getUint32(0)
              if (moofLen + 8 > data.length) return
              const naluData = new Uint8Array(
                data.buffer,
                data.byteOffset + moofLen + 8,
                data.byteLength - moofLen - 8
              )
              if (naluData.length < 5) return

              let isKey = false
              let pos = 0
              while (pos + 4 < naluData.length) {
                const n0 = naluData[pos] ?? 0
                const n1 = naluData[pos + 1] ?? 0
                const n2 = naluData[pos + 2] ?? 0
                const n3 = naluData[pos + 3] ?? 0
                const n4 = naluData[pos + 4] ?? 0
                const nalLen = (n0 << 24) | (n1 << 16) | (n2 << 8) | n3
                const nalType = n4 & 0x1f
                if (nalType === 5 || nalType === 7) {
                  isKey = true
                  break
                }
                pos += 4 + nalLen
              }

              // WebCodecs specification: drop initial delta chunks until first keyframe arrives
              if (!isKey && !s.hasSeenKeyFrame) {
                return
              }
              s.hasSeenKeyFrame = true

              const EncodedVideoChunkClass = (window as any).EncodedVideoChunk
              try {
                s.decoder.decode(
                  new EncodedVideoChunkClass({
                    type: isKey ? "key" : "delta",
                    timestamp: Math.round(performance.now() * 1000),
                    data: naluData,
                  })
                )
              } catch (e) {
                console.warn("[Meridian] decode frame error:", e)
                s.hasSeenKeyFrame = false
              }
              return
            }
          }

          // MSE Fallback path — strictly only if WebCodecs is unavailable
          if (renderMode === "mse" || !hasWebCodecs) {
            s.fpsCount++
            s.pendingBuffers.push(data)
            drainMSE()
            setStreamStatus("loaded")
          }
        }
      }
    }

    connect()

    return () => {
      s.unmounted = true
      if (s.animFrameId) {
        cancelAnimationFrame(s.animFrameId)
        s.animFrameId = null
      }
      clearInterval(statsTimer)
      clearInterval(pingTimer)
      clearInterval(mseTimer)
      clearTimeout(reconnectTimer)
      if (s.ws) {
        s.ws.onclose = null
        s.ws.onerror = null
        s.ws.close()
        s.ws = null
      }
      if (s.decoder) {
        try {
          s.decoder.close()
        } catch {
          // ignore
        }
        s.decoder = null
      }
      if (s.mediaSource && s.mediaSource.readyState === "open") {
        try {
          s.mediaSource.endOfStream()
        } catch {
          // ignore
        }
      }
    }
  }, [wsUrl])

  // Dynamically update stream tuning over WebSocket when scale, fps, or bitrate changes
  useEffect(() => {
    const ws = stateRef.current.ws
    if (ws && ws.readyState === WebSocket.OPEN) {
      try {
        ws.send(
          JSON.stringify({
            op: "tune",
            scale,
            bitrateMbps,
            maxFps: fps,
            keyframeSeconds: 1.0,
          })
        )
      } catch {
        // ignore
      }
    }
  }, [scale, fps, bitrateMbps])

  return (
    <div className={cn("relative h-full w-full overflow-hidden bg-black select-none", className)} style={style}>
      {/* 1. Primary GPU WebCodecs Canvas */}
      <canvas
        ref={canvasRef}
        className={cn(
          "absolute inset-0 h-full w-full object-contain",
          renderMode === "webcodecs" ? "block" : "hidden"
        )}
      />

      {/* 2. Smooth MSE Video Fallback */}
      <video
        ref={videoRef}
        autoPlay
        muted
        playsInline
        className={cn(
          "absolute inset-0 h-full w-full object-contain pointer-events-none",
          renderMode === "mse" ? "block" : "hidden"
        )}
      />

      {/* 3. MJPEG Fallback */}
      {renderMode === "mjpeg" && (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={fallbackUrl}
          alt=""
          className="absolute inset-0 h-full w-full object-contain select-none"
          draggable={false}
          onLoad={() => setStreamStatus("loaded")}
          onError={() => setStreamStatus("error")}
        />
      )}

      {/* Error state */}
      {streamStatus === "error" && (
        <div className="absolute inset-0 flex flex-col items-center justify-center bg-black/80 p-4 text-center">
          <div className="h-2 w-2 rounded-full bg-red-500 animate-ping mb-2" />
          <span className="text-xs text-white/70">Stream offline — reconnecting...</span>
        </div>
      )}
    </div>
  )
})
