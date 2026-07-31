"use client"

import { useEffect, useRef } from "react"
import { cn } from "@/lib/utils"
import { GOLD_THEMED_SCROLLBAR_CLASS } from "@/lib/themed-scrollbar"
import {
  StickToBottom,
  type StickToBottomContext,
  type StickToBottomProps,
  useStickToBottomContext,
} from "use-stick-to-bottom"

export type ChatContainerContext = StickToBottomContext

export type ChatContainerRootProps = {
  children: React.ReactNode
  className?: string
  resize?: StickToBottomProps["resize"]
  initial?: StickToBottomProps["initial"]
  contextRef?: StickToBottomProps["contextRef"]
  onAtBottomChange?: (atBottom: boolean) => void
  onViewportScroll?: (viewport: HTMLDivElement) => void
  onViewportWheel?: (event: WheelEvent, viewport: HTMLDivElement) => void
} & React.HTMLAttributes<HTMLDivElement>

export type ChatContainerContentProps = {
  children: React.ReactNode
  className?: string
  scrollClassName?: string
} & React.HTMLAttributes<HTMLDivElement>

export type ChatContainerScrollAnchorProps = {
  className?: string
  ref?: React.RefObject<HTMLDivElement>
} & React.HTMLAttributes<HTMLDivElement>

export const CHAT_CONTAINER_BOTTOM_REJOIN_TOLERANCE_PX = 2

export function isChatContainerViewportAtBottom(
  viewport: Pick<HTMLDivElement, "clientHeight" | "scrollHeight" | "scrollTop">,
) {
  return (
    viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <=
    CHAT_CONTAINER_BOTTOM_REJOIN_TOLERANCE_PX
  )
}

function ChatContainerRoot({
  children,
  className,
  resize = "smooth",
  initial = "instant",
  contextRef,
  onAtBottomChange,
  onViewportScroll,
  onViewportWheel,
  ...props
}: ChatContainerRootProps) {
  return (
    <StickToBottom
      className={cn("relative min-h-0 min-w-0", className)}
      resize={resize}
      initial={initial}
      contextRef={contextRef}
      role="log"
      {...props}
    >
      {children}
      <ChatContainerLifecycle
        onAtBottomChange={onAtBottomChange}
        onViewportScroll={onViewportScroll}
        onViewportWheel={onViewportWheel}
      />
    </StickToBottom>
  )
}

function ChatContainerContent({
  children,
  className,
  scrollClassName,
  ...props
}: ChatContainerContentProps) {
  return (
    <StickToBottom.Content
      scrollClassName={cn(
        "overflow-y-auto",
        scrollClassName ?? GOLD_THEMED_SCROLLBAR_CLASS,
      )}
      className={cn("flex w-full flex-col", className)}
      {...props}
    >
      {children}
    </StickToBottom.Content>
  )
}

function ChatContainerLifecycle({
  onAtBottomChange,
  onViewportScroll,
  onViewportWheel,
}: Pick<
  ChatContainerRootProps,
  "onAtBottomChange" | "onViewportScroll" | "onViewportWheel"
>) {
  const { isAtBottom, scrollRef, stopScroll } = useStickToBottomContext()
  const escapedByUserRef = useRef(false)

  useEffect(() => {
    onAtBottomChange?.(escapedByUserRef.current ? false : isAtBottom)
  }, [isAtBottom, onAtBottomChange])

  useEffect(() => {
    const viewport = scrollRef.current as HTMLDivElement | null
    if (!viewport) return

    const handleScroll = () => {
      if (
        escapedByUserRef.current &&
        isChatContainerViewportAtBottom(viewport)
      ) {
        escapedByUserRef.current = false
        onAtBottomChange?.(true)
      }
      onViewportScroll?.(viewport)
    }
    const handleWheel = (event: WheelEvent) => {
      if (
        event.deltaY < 0 &&
        viewport.scrollHeight > viewport.clientHeight
      ) {
        escapedByUserRef.current = true
        stopScroll()
        onAtBottomChange?.(false)
      }
      onViewportWheel?.(event, viewport)
    }
    viewport.addEventListener("scroll", handleScroll, { passive: true })
    viewport.addEventListener("wheel", handleWheel, {
      capture: true,
      passive: true,
    })
    return () => {
      viewport.removeEventListener("scroll", handleScroll)
      viewport.removeEventListener("wheel", handleWheel, { capture: true })
    }
  }, [onAtBottomChange, onViewportScroll, onViewportWheel, scrollRef, stopScroll])

  return null
}

function ChatContainerScrollAnchor({
  className,
  ...props
}: ChatContainerScrollAnchorProps) {
  return (
    <div
      className={cn("h-px w-full shrink-0 scroll-mt-4", className)}
      aria-hidden="true"
      {...props}
    />
  )
}

export { ChatContainerRoot, ChatContainerContent, ChatContainerScrollAnchor }
