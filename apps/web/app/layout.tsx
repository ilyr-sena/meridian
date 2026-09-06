import { Geist, Geist_Mono } from "next/font/google"

import "@workspace/ui/globals.css"
import { ThemeProvider } from "@/components/theme-provider"
import { DotGridSpotlight } from "@/components/dot-grid-spotlight"
import { AppSidebar } from "@/components/app-sidebar"
import {
  SidebarProvider,
  SidebarInset,
  SidebarTrigger,
} from "@workspace/ui/components/sidebar"
import { TooltipProvider } from "@workspace/ui/components/tooltip"
import { cn } from "@workspace/ui/lib/utils"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  CirclePowerIcon,
  FocusIcon,
  LockOpen,
  PowerIcon,
  Settings01Icon,
  SmartPhone01Icon,
  VolumeHighIcon,
} from "@hugeicons/core-free-icons"
import { Button } from "@workspace/ui/components/button"

const geist = Geist({ subsets: ["latin"], variable: "--font-sans" })

const fontMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
})

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn(
        "antialiased",
        fontMono.variable,
        "font-sans",
        geist.variable
      )}
    >
      <body>
        <ThemeProvider>
          <TooltipProvider>
            {/*
              DotGridSpotlight: cursor-following illumination over the dot grid.
              Disable with enabled={false}, tune radius/intensity as needed.
            */}
            <DotGridSpotlight enabled={true} radius={220} intensity={0.1} />
            <SidebarProvider>
              <AppSidebar />
              <SidebarInset>
                <div className="group absolute top-2 right-2 z-20 flex hidden h-14 w-fit cursor-pointer items-center justify-start gap-1.5 rounded-[32px] border border-white/5 bg-sidebar/40 pr-6 pl-3 shadow-lg backdrop-blur-[3px] transition-all duration-200 ease-in-out hover:border-white/10">
                  <div className="flex gap-1.5 rounded-full p-2">
                    <HugeiconsIcon
                      icon={SmartPhone01Icon}
                      size={16}
                      strokeWidth={2}
                      className="cursor-pointer text-blue-500 opacity-70 transition-all duration-200 ease-in-out group-hover:opacity-90"
                    />
                  </div>
                  <div className="flex flex-col gap-0">
                    <span className="truncate text-xs font-medium">
                      Devices
                    </span>
                    <span className="truncate text-[0.6875rem] text-muted-foreground">
                      3 available
                    </span>
                  </div>

                  <SidebarTrigger className="hidden" />
                </div>
                {children}
              </SidebarInset>
            </SidebarProvider>
          </TooltipProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
