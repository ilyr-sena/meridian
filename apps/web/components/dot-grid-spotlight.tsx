"use client";

import { useEffect, useRef, useState } from "react";
import { useTheme } from "next-themes";

interface DotGridSpotlightProps {
  /** Enable or disable the cursor spotlight effect entirely. Default: true */
  enabled?: boolean;
  /** Spotlight radius in pixels. Default: 220 */
  radius?: number;
  /** Opacity of the illuminated dots at the cursor center (0-1). Default: 0.15 */
  intensity?: number;
}

/**
 * DotGridSpotlight
 *
 * Renders a fixed spotlight layer on top of the base dot grid.
 * Automatically adapts between dark mode (white dots) and light mode
 * (dark dots) using next-themes.
 */
export function DotGridSpotlight({
  enabled = true,
  radius = 220,
  intensity = 0.15,
}: DotGridSpotlightProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { resolvedTheme } = useTheme();
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  // Track cursor position
  useEffect(() => {
    if (!enabled || !mounted || !ref.current) return;
    const el = ref.current;

    const handleMouseMove = (e: MouseEvent) => {
      el.style.setProperty("--cx", `${e.clientX}px`);
      el.style.setProperty("--cy", `${e.clientY}px`);
      el.style.opacity = "1";
    };

    const handleMouseLeave = () => {
      el.style.opacity = "0";
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseleave", handleMouseLeave);

    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseleave", handleMouseLeave);
    };
  }, [enabled, mounted]);

  if (!enabled || !mounted) return null;

  const isDark = resolvedTheme === "dark";
  const dotColor = isDark
    ? `rgba(255, 255, 255, ${intensity})`
    : `rgba(0, 0, 0, ${intensity + 0.05})`;

  return (
    <div
      ref={ref}
      aria-hidden
      style={
        {
          position: "fixed",
          inset: 0,
          zIndex: 0,
          pointerEvents: "none",
          opacity: 0,
          transition: "opacity 0.4s ease",

          backgroundImage: `radial-gradient(circle, ${dotColor} 1.5px, transparent 1.5px)`,
          backgroundSize: "16px 16px",

          WebkitMaskImage: `radial-gradient(circle ${radius}px at var(--cx, -9999px) var(--cy, -9999px), black 0%, transparent 100%)`,
          maskImage: `radial-gradient(circle ${radius}px at var(--cx, -9999px) var(--cy, -9999px), black 0%, transparent 100%)`,

          ["--cx" as string]: "-9999px",
          ["--cy" as string]: "-9999px",
        } as React.CSSProperties
      }
    />
  );
}
