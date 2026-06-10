"use client"

import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"
import {
  CheckCircle,
  ChevronDown,
  Loader2,
  Settings,
  XCircle,
} from "lucide-react"
import { useState } from "react"

export type ToolParam = { label: string; value: string }

export type ToolPart = {
  type: string
  state:
    | "input-streaming"
    | "input-available"
    | "output-available"
    | "output-error"
  input?: Record<string, unknown>
  orderedInput?: ToolParam[]
  rawInput?: unknown
  output?: unknown
  summary?: string
  toolCallId?: string
  errorText?: string
}

export type ToolLabels = {
  input: string
  output: string
  error: string
  processing: string
  pending: string
  ready: string
  completed: string
}

export type ToolProps = {
  toolPart: ToolPart
  labels: ToolLabels
  defaultOpen?: boolean
  open?: boolean
  className?: string
  icon?: React.ReactNode
  onOpenChange?: (open: boolean) => void
  animated?: boolean
  renderContent?: boolean
}

const Tool = ({ toolPart, labels, defaultOpen = false, open, className, icon, onOpenChange, animated = true, renderContent = true }: ToolProps) => {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen)
  const isOpen = open ?? uncontrolledOpen
  const { state, input, orderedInput, rawInput, output, summary } = toolPart

  const getStateIcon = () => {
    if (icon) return icon
    switch (state) {
      case "input-streaming":
        return <Loader2 className="size-4 animate-spin text-primary" />
      case "input-available":
        return <Settings className="size-4 text-orange-500" />
      case "output-available":
        return <CheckCircle className="size-4 text-emerald-500" />
      case "output-error":
        return <XCircle className="size-4 text-destructive" />
      default:
        return <Settings className="text-muted-foreground size-4" />
    }
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (open === undefined) setUncontrolledOpen(nextOpen)
    onOpenChange?.(nextOpen)
  }

  const getStateBadge = () => {
    const baseClasses = "shrink-0 rounded-full px-2 py-0.5 text-xs font-medium"
    switch (state) {
      case "input-streaming":
        return <span className={cn(baseClasses, "bg-primary/10 text-primary")}>{labels.processing}</span>
      case "input-available":
        return <span className={cn(baseClasses, "bg-orange-500/10 text-orange-600 dark:text-orange-300")}>{labels.ready}</span>
      case "output-available":
        return <span className={cn(baseClasses, "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300")}>{labels.completed}</span>
      case "output-error":
        return <span className={cn(baseClasses, "bg-destructive/10 text-destructive")}>{labels.error}</span>
      default:
        return <span className={cn(baseClasses, "bg-muted text-muted-foreground")}>{labels.pending}</span>
    }
  }

  const formatValue = (value: unknown): string => {
    if (value === null) return "null"
    if (value === undefined) return "undefined"
    if (typeof value === "string") return value
    if (typeof value === "object") return JSON.stringify(value, null, 2)
    return String(value)
  }

  return (
    <div className={cn("border-border min-w-0 max-w-full overflow-hidden rounded-xl border bg-card/75 shadow-sm shadow-background/30", className)}>
      <Collapsible open={isOpen} onOpenChange={handleOpenChange}>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" className="h-auto w-full min-w-0 justify-between overflow-hidden rounded-none px-3 py-2 font-normal hover:bg-muted/20">
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">{getStateIcon()}</span>
              <span className="min-w-0 flex-1 truncate text-left font-mono text-sm">
                <span className="font-semibold text-foreground">{toolPart.type}</span>
                {summary ? <span className="ml-2 text-xs text-muted-foreground">{summary}</span> : null}
              </span>
            </div>
            <span className="ml-3 flex shrink-0 items-center gap-3">
              {getStateBadge()}
              <ChevronDown className={cn("size-4 shrink-0 text-muted-foreground transition-transform", isOpen && "rotate-180")} />
            </span>
          </Button>
        </CollapsibleTrigger>
        {renderContent && isOpen ? (
          <CollapsibleContent className={cn("border-border min-w-0 max-w-full overflow-hidden border-t", animated && "data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down")}>
            <div className="min-w-0 max-w-full space-y-2 overflow-hidden bg-background/50 p-2.5">
              {(orderedInput && orderedInput.length > 0) || (input && Object.keys(input).length > 0) ? (
                <div>
                  <h4 className="text-muted-foreground mb-2 text-xs font-medium uppercase tracking-wide">{labels.input}</h4>
                  {orderedInput && orderedInput.length > 0 ? (
                    <div className="min-w-0 max-w-full space-y-1.5">
                      {orderedInput.map((param, index) => (
                        <div key={`${param.label}-${index}`} className="min-w-0 max-w-full overflow-hidden rounded-lg border bg-background/70 px-2.5 py-1.5 font-mono text-xs">
                          <div className="text-muted-foreground mb-1 truncate">{param.label}</div>
                          <div className="break-all text-foreground [overflow-wrap:anywhere]">{param.value}</div>
                        </div>
                      ))}
                    </div>
                  ) : input && Object.keys(input).length > 0 ? (
                    <div className="grid min-w-0 max-w-full gap-2 sm:grid-cols-2">
                      {Object.entries(input).map(([key, value]) => (
                        <div key={key} className="min-w-0 max-w-full overflow-hidden rounded-lg border bg-background/70 px-2.5 py-1.5 font-mono text-xs">
                          <div className="text-muted-foreground mb-1 truncate">{key}</div>
                          <div className="break-all text-foreground [overflow-wrap:anywhere]">{formatValue(value)}</div>
                        </div>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : rawInput ? (
                <div>
                  <h4 className="text-muted-foreground mb-2 text-xs font-medium uppercase tracking-wide">{labels.input}</h4>
                  <div className="max-h-60 max-w-full overflow-auto rounded-lg border bg-background/70 p-2.5 font-mono text-xs">
                    <pre className="min-w-0 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{formatValue(rawInput)}</pre>
                  </div>
                </div>
              ) : null}

              {output ? (
                <div>
                  <h4 className="text-muted-foreground mb-2 text-xs font-medium uppercase tracking-wide">{labels.output}</h4>
                  <div className="max-h-60 max-w-full overflow-auto rounded-lg border bg-background/70 p-2.5 font-mono text-xs">
                    <pre className="min-w-0 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{formatValue(output)}</pre>
                  </div>
                </div>
              ) : null}

              {state === "output-error" && toolPart.errorText ? (
                <div>
                  <h4 className="mb-2 text-xs font-medium uppercase tracking-wide text-destructive">{labels.error}</h4>
                  <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-sm text-destructive break-words [overflow-wrap:anywhere]">
                    {toolPart.errorText}
                  </div>
                </div>
              ) : null}
            </div>
          </CollapsibleContent>
        ) : null}
      </Collapsible>
    </div>
  )
}

export { Tool }
