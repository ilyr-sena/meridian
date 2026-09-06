"use client"

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@workspace/ui/components/sidebar"
import {
  Avatar,
  AvatarFallback,
  AvatarImage,
} from "@workspace/ui/components/avatar"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  SmartPhone01Icon,
  Activity01Icon,
  Settings01Icon,
  ChevronDoubleCloseIcon,
  BadgeQuestionMarkIcon,
  User02Icon,
  People,
  CreditCardIcon,
  DarkModeIcon,
  ContrastIcon,
  LogoutIcon,
  FullSignalIcon,
  MediumSignalIcon,
  LowSignalIcon,
} from "@hugeicons/core-free-icons"
import { MeridianIcon, MeridianText } from "./meridian-logo"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@workspace/ui/components/dropdown-menu"
import { Button } from "@workspace/ui/components/button"
import { useState, useEffect } from "react"
import { useLiveQuery } from "@/hooks/use-live-query"

// Main navigation items
const navItems = [
  {
    title: "Devices",
    url: "#",
    icon: SmartPhone01Icon,
    onClick: () => {
      if (typeof window !== "undefined") {
        window.dispatchEvent(new Event("open-devices-drawer"))
      }
    },
  },
  {
    title: "Sessions",
    url: "#",
    icon: Activity01Icon,
    onClick: () => {
      if (typeof window !== "undefined") {
        window.dispatchEvent(new Event("open-sessions-drawer"))
      }
    },
  },
]

// Bottom utility items — rendered above the user card
const bottomItems = [
  {
    title: "Help",
    url: "#",
    icon: BadgeQuestionMarkIcon,
  },
  {
    title: "Settings",
    url: "#",
    icon: Settings01Icon,
  },
]

export function AppSidebar() {
  const { toggleSidebar } = useSidebar()
  const [networkStatus, setNetworkStatus] = useState(2)
  const { data: dbUserRaw } = useLiveQuery<{ user: { name: string; team: { name: string } | null } }>({
    url: "/api/users/me",
    collection: "users",
  })
  const { data: dbSessionsRaw } = useLiveQuery<{
    activeSession: { _id: string; id: string; devices: string[]; started: string } | null
  }>({
    url: "/api/sessions",
    collection: "sessions",
  })
  const { data: dbDevicesRaw } = useLiveQuery<{ devices: { _id: string; name: string }[] }>({
    url: "/api/devices",
    collection: "devices",
  })
  const dbUser = dbUserRaw?.user ?? null
  const activeSession = dbSessionsRaw?.activeSession ?? null
  const dbDevices = dbDevicesRaw?.devices ?? []

  // Real-time elapsed timer for sidebar session card
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

  return (
    <Sidebar
      variant="floating"
      collapsible="icon"
      onClick={toggleSidebar}
      className="cursor-pointer"
    >
      <SidebarContent>
        <SidebarGroup>
          {/* Logo header — icon and text transition independently */}
          <div className="-mt-1 flex h-16 items-center gap-2 overflow-hidden px-2.5 py-4">
            {/* Icon: always visible, always brand-colored, never moves */}
            <MeridianIcon className="h-[18px] w-auto shrink-0 text-[#5266e7]" />
            {/* Text: fades + clips out when sidebar collapses, no layout shift */}
            <div className="max-w-[200px] overflow-hidden opacity-100 transition-[max-width,opacity] duration-300 ease-in-out group-data-[collapsible=icon]:max-w-0 group-data-[collapsible=icon]:opacity-0">
              <MeridianText className="h-[14px] w-auto text-foreground" />
            </div>
          </div>
          <SidebarGroupLabel className="select-none">
            Control Center
          </SidebarGroupLabel>
          <SidebarGroupContent className="">
            <SidebarMenu>
              {navItems.map((item) => (
                <SidebarMenuItem key={item.title}>
                  <SidebarMenuButton
                    tooltip={item.title}
                    render={
                      <a
                        href={item.url}
                        onClick={(e) => {
                          e.stopPropagation()
                          if (item.onClick) {
                            e.preventDefault()
                            item.onClick()
                          }
                        }}
                      >
                        <HugeiconsIcon
                          icon={item.icon}
                          strokeWidth={2}
                          className="opacity-70"
                        />
                        <span className="select-none">{item.title}</span>
                      </a>
                    }
                  />
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <SidebarMenu className="mb-2">
          {bottomItems.map((item) => (
            <SidebarMenuItem key={item.title}>
              <SidebarMenuButton
                tooltip={item.title}
                render={
                  <a href={item.url} onClick={(e) => e.stopPropagation()}>
                    <HugeiconsIcon
                      icon={item.icon}
                      strokeWidth={2}
                      className="opacity-50"
                    />
                    <span className="text-foreground/50 select-none">
                      {item.title}
                    </span>
                  </a>
                }
              />
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
        {activeSession && (
          <div
            id="session-status"
            className={`flex h-fit w-full cursor-pointer items-center gap-2 rounded-2xl border py-2 pr-3 pl-2 transition-all duration-200 ease-in-out group-data-[collapsible=icon]:p-1 ${networkStatus === 3 ? "border-emerald-500/5 bg-emerald-500/4" : networkStatus === 2 ? "border-amber-500/5 bg-amber-400/2" : "border-rose-400/8 bg-rose-400/5"}`}
            onClick={(e) => {
              e.stopPropagation()
              if (typeof window !== "undefined") {
                window.dispatchEvent(new Event("open-sessions-drawer"))
              }
            }}
          >
            <div
              className={`flex aspect-square size-9.5 cursor-pointer items-center justify-center rounded-xl border ${networkStatus === 3 ? "border-emerald-500/10 bg-emerald-500/8 text-emerald-600" : networkStatus === 2 ? "border-amber-500/10 bg-amber-500/5 text-amber-500" : "border-rose-400/15 bg-rose-400/7 text-rose-600"}`}
              onClick={(e) => {
                setNetworkStatus(
                  networkStatus === 3 ? 1 : networkStatus === 1 ? 2 : 3
                )
                e.stopPropagation()
              }}
            >
              <HugeiconsIcon
                icon={
                  networkStatus === 3
                    ? FullSignalIcon
                    : networkStatus === 2
                      ? MediumSignalIcon
                      : LowSignalIcon
                }
                strokeWidth={1.5}
                className=""
              />
            </div>
            <div className="flex max-w-[300px] min-w-0 flex-1 flex-col gap-0 overflow-hidden opacity-100 transition-[max-width,opacity] duration-200 ease-in-out group-data-[collapsible=icon]:max-w-0 group-data-[collapsible=icon]:opacity-0">
              <div className="flex items-center justify-between">
                <span className="truncate text-xs font-medium select-none">
                  Current Session
                </span>
                <span className="truncate overflow-hidden font-mono text-xs opacity-100 transition-[max-width,opacity] duration-200 ease-in-out select-none group-data-[collapsible=icon]:max-w-0 group-data-[collapsible=icon]:opacity-0">
                  {elapsed}
                </span>
              </div>
              <span className="flex w-full gap-1.25 overflow-hidden [mask-image:linear-gradient(to_right,black_85%,transparent)] text-[11px] whitespace-nowrap text-muted-foreground select-none">
                {activeSession.devices.length} Device{activeSession.devices.length !== 1 ? "s" : ""} <p className="opacity-80">&bull;</p> {activeSession.devices.map((did) => dbDevices.find((d) => d._id === did)?.name).filter(Boolean).join(", ") || ""}
              </span>
            </div>
          </div>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger>
            <div
              className="group/footer flex w-full cursor-pointer items-center justify-start gap-2 rounded-full border border-white/0 py-2 pr-3.5 pl-2 transition-all duration-200 ease-in-out group-data-[collapsible=icon]:gap-0 group-data-[collapsible=icon]:p-[5px] hover:border-border hover:bg-secondary/70"
              onClick={(e) => e.stopPropagation()}
            >
                <Avatar className="h-9 w-9 shrink-0">
                  <AvatarImage
                    src="https://upload.wikimedia.org/wikipedia/commons/7/7c/Profile_avatar_placeholder_large"
                    alt={dbUser?.name ?? "User"}
                  />
                  <AvatarFallback>
                    {dbUser?.name?.split(" ").map((n) => n[0]).join("").slice(0, 2).toUpperCase() ?? "U"}
                  </AvatarFallback>
                </Avatar>
                <div className="flex max-w-[200px] flex-1 flex-col items-start justify-start overflow-hidden opacity-100 transition-[max-width,opacity] duration-200 ease-in-out select-none group-data-[collapsible=icon]:max-w-0 group-data-[collapsible=icon]:opacity-0">
                  <div className="flex items-center gap-1.5">
                    <span className="w-full truncate text-xs font-medium">
                      {dbUser?.name ?? "User"}
                    </span>
                  </div>
                  <span className="w-full truncate text-start text-[11px] text-muted-foreground">
                    {dbUser?.team?.name ?? "Team"}
                  </span>
                </div>
              <div className="max-w-[20px] shrink-0 overflow-hidden opacity-100 transition-[max-width,opacity] duration-200 ease-in-out group-data-[collapsible=icon]:max-w-0 group-data-[collapsible=icon]:opacity-0">
                <HugeiconsIcon
                  icon={ChevronDoubleCloseIcon}
                  strokeWidth={2}
                  size={16}
                  className="rotate-270 opacity-70 transition-opacity duration-200 ease-in-out group-hover/footer:opacity-100"
                />
              </div>
            </div>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="tracking-[-0.01em]">
            <DropdownMenuGroup>
              <DropdownMenuLabel>Account</DropdownMenuLabel>
              <DropdownMenuItem>
                <HugeiconsIcon
                  icon={User02Icon}
                  strokeWidth={2}
                  className="opacity-70"
                />
                Profile
              </DropdownMenuItem>
              <DropdownMenuItem>
                <HugeiconsIcon
                  icon={People}
                  strokeWidth={2}
                  className="opacity-70"
                />
                Team
              </DropdownMenuItem>
              <DropdownMenuItem>
                <HugeiconsIcon
                  icon={CreditCardIcon}
                  strokeWidth={2}
                  className="opacity-70"
                />
                Plan
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem>
                <HugeiconsIcon
                  icon={ContrastIcon}
                  strokeWidth={2}
                  className="opacity-70"
                />
                Theme
              </DropdownMenuItem>
              <DropdownMenuItem className="text-destructive focus:!bg-destructive/10 focus:!text-destructive">
                <HugeiconsIcon
                  icon={LogoutIcon}
                  strokeWidth={2}
                  className="opacity-90"
                />
                Log out
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarFooter>
    </Sidebar>
  )
}
