import * as React from "react"
import { XIcon } from "lucide-react"
import { Dialog as SheetPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"
import { PortalContainerContext } from "@/lib/portal-container"

const sheetResizeStoragePrefix = "gold-band:sheet-size:"
const defaultSheetMinSize = 360
const defaultSheetMaxSize = 1280
const defaultSheetViewportMargin = 16
const sheetResizeStep = 24

type SheetSide = "top" | "right" | "bottom" | "left"

function Sheet({ ...props }: React.ComponentProps<typeof SheetPrimitive.Root>) {
  return <SheetPrimitive.Root data-slot="sheet" {...props} />
}

function SheetTrigger({
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Trigger>) {
  return <SheetPrimitive.Trigger data-slot="sheet-trigger" {...props} />
}

function SheetClose({
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Close>) {
  return <SheetPrimitive.Close data-slot="sheet-close" {...props} />
}

function SheetPortal({
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Portal>) {
  return <SheetPrimitive.Portal data-slot="sheet-portal" {...props} />
}

function SheetOverlay({
  className,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Overlay>) {
  return (
    <SheetPrimitive.Overlay
      data-slot="sheet-overlay"
      className={cn(
        "fixed inset-0 z-50 bg-black/50 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:fade-in-0",
        className
      )}
      {...props}
    />
  )
}

function clampSheetSize(value: number, minSize: number, maxSize: number) {
  const effectiveMinSize = Math.min(minSize, maxSize)
  return Math.min(Math.max(value, effectiveMinSize), maxSize)
}

function resolveSheetViewportMaxSize(_minSize: number, maxSize: number, viewportMargin: number) {
  if (typeof window === "undefined") return maxSize
  return Math.min(maxSize, Math.max(240, window.innerWidth - viewportMargin))
}

function readStoredSheetSize(key: string) {
  if (typeof window === "undefined") return null
  try {
    const stored = Number(window.localStorage.getItem(key))
    return Number.isFinite(stored) && stored > 0 ? stored : null
  } catch {
    return null
  }
}

function resolveInitialSheetSize(options: {
  resizeEnabled: boolean
  storageKey: string | null
  defaultSize?: number
  clampSize: (value: number) => number
}) {
  if (!options.resizeEnabled) return null
  const storedSize = options.storageKey ? readStoredSheetSize(options.storageKey) : null
  if (storedSize !== null) return options.clampSize(storedSize)
  if (typeof options.defaultSize === "number") return options.clampSize(options.defaultSize)
  return null
}

function writeStoredSheetSize(key: string, value: number) {
  if (typeof window === "undefined") return
  try {
    window.localStorage.setItem(key, String(Math.round(value)))
  } catch {}
}

function fallbackSheetResizeKey(side: SheetSide, className: string | undefined, style: React.CSSProperties | undefined) {
  if (typeof window === "undefined") return `${sheetResizeStoragePrefix}${side}`
  const width = typeof style?.width === "number" ? String(style.width) : String(style?.width ?? "")
  const maxWidth = typeof style?.maxWidth === "number" ? String(style.maxWidth) : String(style?.maxWidth ?? "")
  return `${sheetResizeStoragePrefix}${window.location.pathname}:${side}:${width}:${maxWidth}:${className ?? ""}`
}

function SheetContent({
  className,
  children,
  side = "right",
  showCloseButton = true,
  showOverlay = true,
  closeLabel = "Close",
  resizable = true,
  resizeStorageKey,
  defaultSize,
  minSize = defaultSheetMinSize,
  maxSize = defaultSheetMaxSize,
  viewportMargin = defaultSheetViewportMargin,
  style,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Content> & {
  side?: SheetSide
  showCloseButton?: boolean
  showOverlay?: boolean
  closeLabel?: string
  resizable?: boolean
  resizeStorageKey?: string
  defaultSize?: number
  minSize?: number
  maxSize?: number
  viewportMargin?: number
}) {
  const contentRef = React.useRef<HTMLDivElement | null>(null)
  const [portalContainerEl, setPortalContainerEl] = React.useState<HTMLElement | null>(null)
  const setContentRef = React.useCallback((node: HTMLDivElement | null) => {
    contentRef.current = node;
    setPortalContainerEl(node);
  }, [])
  const dragCleanupRef = React.useRef<(() => void) | null>(null)
  const resizeEnabled = resizable && (side === "right" || side === "left")

  const clampSize = React.useCallback(
    (value: number) => clampSheetSize(value, minSize, resolveSheetViewportMaxSize(minSize, maxSize, viewportMargin)),
    [maxSize, minSize, viewportMargin]
  )

  const storageKey = React.useMemo(() => {
    if (!resizeEnabled) return null
    if (resizeStorageKey) return `${sheetResizeStoragePrefix}${resizeStorageKey}`
    return fallbackSheetResizeKey(side, className, style)
  }, [className, resizeEnabled, resizeStorageKey, side, style])

  const [size, setSize] = React.useState<number | null>(() => resolveInitialSheetSize({
    resizeEnabled,
    storageKey,
    defaultSize,
    clampSize,
  }))
  const [isResizing, setIsResizing] = React.useState(false)
  const [hoverReady, setHoverReady] = React.useState(false)

  React.useEffect(() => {
    setSize(resolveInitialSheetSize({
      resizeEnabled,
      storageKey,
      defaultSize,
      clampSize,
    }))
  }, [clampSize, defaultSize, resizeEnabled, storageKey])

  React.useLayoutEffect(() => {
    if (!resizeEnabled || size !== null || !contentRef.current) return
    const measured = contentRef.current.getBoundingClientRect().width
    if (measured > 0) setSize(clampSize(measured))
  }, [clampSize, resizeEnabled, size])

  React.useEffect(() => {
    if (!resizeEnabled || size === null || !storageKey) return
    writeStoredSheetSize(storageKey, size)
  }, [resizeEnabled, size, storageKey])

  React.useEffect(() => {
    if (!resizeEnabled) return
    const handleResize = () => {
      setSize((current) => (current === null ? current : clampSize(current)))
    }
    window.addEventListener("resize", handleResize)
    return () => window.removeEventListener("resize", handleResize)
  }, [clampSize, resizeEnabled])

  React.useEffect(() => {
    return () => {
      dragCleanupRef.current?.()
      dragCleanupRef.current = null
    }
  }, [])

  React.useEffect(() => {
    if (!resizeEnabled) {
      setHoverReady(false)
      return
    }
    setHoverReady(false)
    const timer = window.setTimeout(() => setHoverReady(true), 180)
    return () => window.clearTimeout(timer)
  }, [resizeEnabled, storageKey])

  const setClampedSize = React.useCallback(
    (value: number) => {
      setSize(clampSize(value))
    },
    [clampSize]
  )

  const resizeFromClientX = React.useCallback(
    (clientX: number) => {
      if (typeof window === "undefined") return
      setClampedSize(side === "right" ? window.innerWidth - clientX : clientX)
    },
    [setClampedSize, side]
  )

  const startResize = React.useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (!resizeEnabled) return
      event.preventDefault()
      event.stopPropagation()
      const target = event.currentTarget
      const pointerId = event.pointerId
      const previousCursor = document.body.style.cursor
      const previousUserSelect = document.body.style.userSelect
      document.body.style.cursor = "ew-resize"
      document.body.style.userSelect = "none"
      setIsResizing(true)
      target.setPointerCapture(pointerId)
      resizeFromClientX(event.clientX)

      const stop = () => {
        setIsResizing(false)
        document.body.style.cursor = previousCursor
        document.body.style.userSelect = previousUserSelect
        window.removeEventListener("pointermove", handlePointerMove)
        window.removeEventListener("pointerup", handlePointerUp)
        window.removeEventListener("pointercancel", handlePointerUp)
        if (target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId)
        if (dragCleanupRef.current === stop) dragCleanupRef.current = null
      }

      const handlePointerMove = (moveEvent: PointerEvent) => {
        if (moveEvent.pointerId !== pointerId) return
        resizeFromClientX(moveEvent.clientX)
      }

      const handlePointerUp = (upEvent: PointerEvent) => {
        if (upEvent.pointerId !== pointerId) return
        stop()
      }

      dragCleanupRef.current?.()
      dragCleanupRef.current = stop
      window.addEventListener("pointermove", handlePointerMove)
      window.addEventListener("pointerup", handlePointerUp)
      window.addEventListener("pointercancel", handlePointerUp)
    },
    [resizeEnabled, resizeFromClientX]
  )

  const handleResizeKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (!resizeEnabled) return
      const direction = side === "right" ? -1 : 1
      if (event.key === "ArrowLeft") {
        event.preventDefault()
        setSize((current) => clampSize((current ?? defaultSize ?? minSize) + sheetResizeStep * direction))
        return
      }
      if (event.key === "ArrowRight") {
        event.preventDefault()
        setSize((current) => clampSize((current ?? defaultSize ?? minSize) - sheetResizeStep * direction))
        return
      }
      if (event.key === "Home") {
        event.preventDefault()
        setSize(minSize)
        return
      }
      if (event.key === "End") {
        event.preventDefault()
        setSize(resolveSheetViewportMaxSize(minSize, maxSize, viewportMargin))
      }
    },
    [clampSize, defaultSize, maxSize, minSize, resizeEnabled, side, viewportMargin]
  )

  const resolvedStyle = React.useMemo<React.CSSProperties>(() => {
    if (!resizeEnabled || size === null) return style ?? {}
    return {
      ...(style ?? {}),
      width: `${size}px`,
      maxWidth: `${size}px`,
    }
  }, [resizeEnabled, size, style])

  return (
    <SheetPortal>
      {showOverlay ? <SheetOverlay /> : null}
      <SheetPrimitive.Content
        ref={setContentRef}
        data-slot="sheet-content"
        className={cn(
          "fixed z-50 flex flex-col gap-4 bg-background shadow-lg transition ease-in-out data-[state=closed]:animate-out data-[state=closed]:duration-300 data-[state=open]:animate-in data-[state=open]:duration-500",
          side === "right" &&
            "inset-y-0 right-0 h-full w-3/4 border-l data-[state=closed]:slide-out-to-right data-[state=open]:slide-in-from-right sm:max-w-sm",
          side === "left" &&
            "inset-y-0 left-0 h-full w-3/4 border-r data-[state=closed]:slide-out-to-left data-[state=open]:slide-in-from-left sm:max-w-sm",
          side === "top" &&
            "inset-x-0 top-0 h-auto border-b data-[state=closed]:slide-out-to-top data-[state=open]:slide-in-from-top",
          side === "bottom" &&
            "inset-x-0 bottom-0 h-auto border-t data-[state=closed]:slide-out-to-bottom data-[state=open]:slide-in-from-bottom",
          className
        )}
        style={resolvedStyle}
        {...props}
      >
        <PortalContainerContext.Provider value={portalContainerEl}>
          {children}
        </PortalContainerContext.Provider>
        {showCloseButton && (
          <SheetPrimitive.Close className="absolute top-4 right-4 rounded-xs opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:ring-2 focus:ring-ring focus:ring-offset-2 focus:outline-hidden disabled:pointer-events-none data-[state=open]:bg-secondary">
            <XIcon className="size-4" />
            <span className="sr-only">{closeLabel}</span>
          </SheetPrimitive.Close>
        )}
        {resizeEnabled ? (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize panel"
            tabIndex={0}
            className={cn(
              "group absolute inset-y-0 z-10 w-3 cursor-ew-resize touch-none outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
              side === "right" ? "left-0" : "right-0"
            )}
            onPointerDown={startResize}
            onKeyDown={handleResizeKeyDown}
          >
            <div
              className={cn(
                "absolute top-0 bottom-0 left-1/2 w-px -translate-x-1/2 bg-transparent transition-colors duration-150",
                hoverReady && "group-hover:bg-border/35 group-focus-visible:bg-border/55",
                isResizing && "bg-primary/65"
              )}
            />
            <div
              className={cn(
                "absolute top-1/2 left-1/2 h-14 w-1 -translate-x-1/2 -translate-y-1/2 rounded-full bg-transparent transition-all duration-150",
                hoverReady && "group-hover:h-16 group-hover:bg-border/45 group-focus-visible:bg-border/65",
                isResizing && "h-18 w-1.5 bg-primary"
              )}
            />
          </div>
        ) : null}
      </SheetPrimitive.Content>
    </SheetPortal>
  )
}

function SheetHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sheet-header"
      className={cn("flex flex-col gap-1.5 p-4", className)}
      {...props}
    />
  )
}

function SheetFooter({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sheet-footer"
      className={cn("mt-auto flex flex-col gap-2 p-4", className)}
      {...props}
    />
  )
}

function SheetTitle({
  className,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Title>) {
  return (
    <SheetPrimitive.Title
      data-slot="sheet-title"
      className={cn("font-semibold text-foreground", className)}
      {...props}
    />
  )
}

function SheetDescription({
  className,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Description>) {
  return (
    <SheetPrimitive.Description
      data-slot="sheet-description"
      className={cn("text-sm text-muted-foreground", className)}
      {...props}
    />
  )
}

export {
  Sheet,
  SheetTrigger,
  SheetClose,
  SheetContent,
  SheetHeader,
  SheetFooter,
  SheetTitle,
  SheetDescription,
}
