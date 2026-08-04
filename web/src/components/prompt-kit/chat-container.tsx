"use client"

import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react"
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
const CHAT_CONTAINER_FOLLOW_RECOVERY_DELAY_MS = 4

const CHAT_CONTAINER_SCROLL_UP_KEYS = new Set([
  "ArrowUp",
  "Home",
  "PageUp",
])

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
      role="log"
      {...props}
    >
      {children}
      <ChatContainerLifecycle
        contextRef={contextRef}
        initialFollowing={initial !== false}
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
  contextRef,
  initialFollowing,
  onAtBottomChange,
  onViewportScroll,
  onViewportWheel,
}: Pick<
  ChatContainerRootProps,
  "contextRef" | "onAtBottomChange" | "onViewportScroll" | "onViewportWheel"
> & { initialFollowing: boolean }) {
  const stickContext = useStickToBottomContext()
  const {
    contentRef,
    scrollRef,
    scrollToBottom: libraryScrollToBottom,
    stopScroll: libraryStopScroll,
  } = stickContext
  const [isFollowing, setIsFollowing] = useState(initialFollowing)
  const isFollowingRef = useRef(initialFollowing)
  const pointerScrollingRef = useRef(false)
  const lastScrollTopRef = useRef<number | null>(null)
  const recoveryTimerRef = useRef<number | null>(null)
  const recoveryFrameRef = useRef<number | null>(null)

  const updateFollowIntent = useCallback((following: boolean) => {
    isFollowingRef.current = following
    setIsFollowing((current) => current === following ? current : following)
  }, [])

  const stopScroll = useCallback(() => {
    updateFollowIntent(false)
    libraryStopScroll()
  }, [libraryStopScroll, updateFollowIntent])

  const scrollToBottom = useCallback<StickToBottomContext["scrollToBottom"]>(
    (options) => {
      updateFollowIntent(true)
      return libraryScrollToBottom(options)
    },
    [libraryScrollToBottom, updateFollowIntent],
  )

  const exposedContext = useMemo<ChatContainerContext>(() => ({
    contentRef: stickContext.contentRef,
    scrollRef: stickContext.scrollRef,
    scrollToBottom,
    stopScroll,
    isAtBottom: isFollowing,
    escapedFromLock: !isFollowing,
    state: stickContext.state,
    get targetScrollTop() {
      return stickContext.targetScrollTop
    },
    set targetScrollTop(targetScrollTop) {
      stickContext.targetScrollTop = targetScrollTop
    },
  }), [isFollowing, scrollToBottom, stickContext, stopScroll])

  useImperativeHandle(contextRef, () => exposedContext, [exposedContext])

  useEffect(() => {
    onAtBottomChange?.(isFollowing)
  }, [isFollowing, onAtBottomChange])

  const scheduleFollowRecovery = useCallback(() => {
    if (!isFollowingRef.current || recoveryTimerRef.current !== null) return
    recoveryTimerRef.current = window.setTimeout(() => {
      recoveryTimerRef.current = null
      recoveryFrameRef.current = requestAnimationFrame(() => {
        recoveryFrameRef.current = null
        if (!isFollowingRef.current) return
        const viewport = scrollRef.current as HTMLDivElement | null
        if (
          viewport &&
          (!isChatContainerViewportAtBottom(viewport) ||
            !stickContext.state.isAtBottom)
        ) {
          void libraryScrollToBottom({ animation: "instant" })
        }
      })
    }, CHAT_CONTAINER_FOLLOW_RECOVERY_DELAY_MS)
  }, [libraryScrollToBottom, scrollRef, stickContext.state])

  useEffect(() => {
    const viewport = scrollRef.current as HTMLDivElement | null
    if (!viewport) return
    lastScrollTopRef.current = viewport.scrollTop

    const handleScroll = () => {
      const previousScrollTop = lastScrollTopRef.current
      const currentScrollTop = viewport.scrollTop
      lastScrollTopRef.current = currentScrollTop
      if (
        pointerScrollingRef.current &&
        previousScrollTop !== null &&
        currentScrollTop < previousScrollTop
      ) {
        stopScroll()
      }
      if (
        !isFollowingRef.current &&
        isChatContainerViewportAtBottom(viewport)
      ) {
        updateFollowIntent(true)
      }
      onViewportScroll?.(viewport)
      scheduleFollowRecovery()
    }
    const handleWheel = (event: WheelEvent) => {
      if (
        event.deltaY < 0 &&
        viewport.scrollHeight > viewport.clientHeight
      ) {
        stopScroll()
      }
      onViewportWheel?.(event, viewport)
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      const scrollsUp = CHAT_CONTAINER_SCROLL_UP_KEYS.has(event.key)
        || (event.key === " " && event.shiftKey)
      if (scrollsUp && viewport.scrollHeight > viewport.clientHeight) {
        stopScroll()
      }
    }
    const handlePointerDown = () => {
      pointerScrollingRef.current = true
    }
    const handlePointerEnd = () => {
      pointerScrollingRef.current = false
    }
    viewport.addEventListener("scroll", handleScroll, { passive: true })
    viewport.addEventListener("wheel", handleWheel, {
      capture: true,
      passive: true,
    })
    viewport.addEventListener("keydown", handleKeyDown, { capture: true })
    viewport.addEventListener("pointerdown", handlePointerDown, {
      capture: true,
      passive: true,
    })
    window.addEventListener("pointerup", handlePointerEnd, { passive: true })
    window.addEventListener("pointercancel", handlePointerEnd, { passive: true })
    return () => {
      viewport.removeEventListener("scroll", handleScroll)
      viewport.removeEventListener("wheel", handleWheel, { capture: true })
      viewport.removeEventListener("keydown", handleKeyDown, { capture: true })
      viewport.removeEventListener("pointerdown", handlePointerDown, { capture: true })
      window.removeEventListener("pointerup", handlePointerEnd)
      window.removeEventListener("pointercancel", handlePointerEnd)
    }
  }, [
    onViewportScroll,
    onViewportWheel,
    scheduleFollowRecovery,
    scrollRef,
    stopScroll,
    updateFollowIntent,
  ])

  useEffect(() => {
    const content = contentRef.current
    if (!content) return
    const observer = new ResizeObserver(scheduleFollowRecovery)
    observer.observe(content)
    return () => observer.disconnect()
  }, [contentRef, scheduleFollowRecovery])

  useEffect(() => () => {
    if (recoveryTimerRef.current !== null) {
      window.clearTimeout(recoveryTimerRef.current)
    }
    if (recoveryFrameRef.current !== null) {
      cancelAnimationFrame(recoveryFrameRef.current)
    }
  }, [])

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
