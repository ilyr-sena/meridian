import { PhoneStage } from "@/components/phone-stage"
import { Button } from "@workspace/ui/components/button"

export default function Page() {
  return (
    <div className="relative flex h-svh w-full gap-2 overflow-hidden p-2">
      <div className="flex min-h-0 w-full flex-1 justify-center gap-19">
        <PhoneStage className="md:-translate-x-[calc((var(--sidebar-width-icon)+1rem)/2)]" />
      </div>
      <div className="mt-18 flex hidden max-w-md min-w-0 flex-col gap-4 text-sm leading-relaxed">
        <div>
          <h1 className="font-medium">Project ready!</h1>
          <p>You may now add components and start building.</p>
          <p>We&apos;ve already added the button component for you.</p>
          <Button className="mt-2">Button</Button>
        </div>
        <div className="font-mono text-xs text-muted-foreground uppercase">
          (Press <kbd>d</kbd> to toggle dark mode)
        </div>
      </div>
    </div>
  )
}
