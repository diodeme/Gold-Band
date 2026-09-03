"use client"

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react"
import { cn } from "@/lib/utils"
import {
  isAcpStreamingDiagnosticsEnabled,
  recordAcpStreamingDiagnostic,
} from "@/lib/acp-streaming-diagnostics"
import { GOLD_THEMED_SCROLLBAR_CLASS } from "@/lib/themed-scrollbar"
import {
  StickToBottom,
  type StickToBottomContext,
  type StickToBottomProps,
  useStickToBottomContext,
} from "use-stick-to-bottom"

export type ChatContainerContentExpansionToken = number

export type ChatContainerContentExpansionController = {
  beginContentExpansion: () => ChatContainerContentExpansionToken | null
  endContentExpansion: (
    token: ChatContainerContentExpansionToken | null,
  ) => boolean
  scrollRef: StickToBottomContext["scrollRef"]
  compensateContentAnchor: (delta: number) => boolean
}

export type ChatContainerContext = StickToBottomContext &
  ChatContainerContentExpansionController

const ChatContainerContentExpansionContext =
  createContext<ChatContainerContentExpansionController | null>(null)

export function useOptionalChatContainerContentExpansion() {
  return useContext(ChatContainerContentExpansionContext)
}

export type ChatContainerRootProps = {
  children: React.ReactNode
  className?: string
  resize?: StickToBottomProps["resize"]
  initial?: StickToBottomProps["initial"]
  contextRef?: React.Ref<ChatContainerContext>
  onAtBottomChange?: (atBottom: boolean) => void
  onViewportScroll?: (viewport: HTMLDivElement) => void
  onViewportUserScroll?: (viewport: HTMLDivElement) => void
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
const CHAT_CONTAINER_DIAGNOSTIC_SAMPLE_MS = 500

const CHAT_CONTAINER_SCROLL_UP_KEYS = new Set([
  "ArrowUp",
  "Home",
  "PageUp",
])

const CHAT_CONTAINER_SCROLL_KEYS = new Set([
  ...CHAT_CONTAINER_SCROLL_UP_KEYS,
  "ArrowDown",
  "End",
  "PageDown",
  " ",
])

export function isChatContainerViewportAtBottom(
  viewport: Pick<HTMLDivElement, "clientHeight" | "scrollHeight" | "scrollTop">,
) {
  return (
    viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <=
    CHAT_CONTAINER_BOTTOM_REJOIN_TOLERANCE_PX
  )
}

export function alignChatContainerViewportToBottomBeforePaint(
  viewport: Pick<HTMLElement, "clientHeight" | "scrollHeight" | "scrollTop">,
) {
  viewport.scrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight)
}

function ChatContainerRoot({
  children,
  className,
  resize = "smooth",
  initial = "instant",
  contextRef,
  onAtBottomChange,
  onViewportScroll,
  onViewportUserScroll,
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
      <ChatContainerLifecycle
        contextRef={contextRef}
        initialFollowing={initial !== false}
        onAtBottomChange={onAtBottomChange}
        onViewportScroll={onViewportScroll}
        onViewportUserScroll={onViewportUserScroll}
      >
        {children}
      </ChatContainerLifecycle>
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
  children,
  contextRef,
  initialFollowing,
  onAtBottomChange,
  onViewportScroll,
  onViewportUserScroll,
}: Pick<
  ChatContainerRootProps,
  "children" | "contextRef" | "onAtBottomChange" | "onViewportScroll" | "onViewportUserScroll"
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
  const nextContentExpansionTokenRef = useRef(0)
  const contentExpansionTokensRef = useRef<Set<number> | null>(null)
  const contentExpansionRestoreFrameRef = useRef<number | null>(null)
  const contentAnchorCompensationFrameRef = useRef<number | null>(null)
  const contentAnchorCompensationActiveRef = useRef(false)
  const initialViewportAlignedRef = useRef(false)
  const lastLayoutDiagnosticAtRef = useRef(0)
  const layoutDiagnosticRef = useRef({
    callbackCount: 0,
    durationMs: 0,
    heightDelta: 0,
    longestCallbackMs: 0,
    maxHeightDelta: 0,
  })

  useLayoutEffect(() => {
    if (initialViewportAlignedRef.current || !initialFollowing) return
    const viewport = scrollRef.current
    if (!viewport) return
    initialViewportAlignedRef.current = true
    alignChatContainerViewportToBottomBeforePaint(viewport)
  }, [initialFollowing, scrollRef])
  const lastFollowDiagnosticAtRef = useRef(0)
  const followDiagnosticRef = useRef({
    checkCount: 0,
    callbackDurationMs: 0,
    longestCallbackMs: 0,
    scrollWriteCount: 0,
  })

  const updateFollowIntent = useCallback((following: boolean) => {
    isFollowingRef.current = following
    setIsFollowing((current) => current === following ? current : following)
  }, [])

  const cancelContentExpansionRestore = useCallback(() => {
    contentExpansionTokensRef.current = null
    if (contentExpansionRestoreFrameRef.current !== null) {
      cancelAnimationFrame(contentExpansionRestoreFrameRef.current)
      contentExpansionRestoreFrameRef.current = null
    }
  }, [])

  const stopScroll = useCallback(() => {
    cancelContentExpansionRestore()
    updateFollowIntent(false)
    libraryStopScroll()
  }, [cancelContentExpansionRestore, libraryStopScroll, updateFollowIntent])

  const scrollToBottom = useCallback<StickToBottomContext["scrollToBottom"]>(
    (options) => {
      cancelContentExpansionRestore()
      updateFollowIntent(true)
      return libraryScrollToBottom(options)
    },
    [cancelContentExpansionRestore, libraryScrollToBottom, updateFollowIntent],
  )

  const beginContentExpansion = useCallback(() => {
    let expansionTokens = contentExpansionTokensRef.current
    if (!expansionTokens) {
      if (!isFollowingRef.current) return null
      expansionTokens = new Set<number>()
      contentExpansionTokensRef.current = expansionTokens
      updateFollowIntent(false)
      libraryStopScroll()
    }
    const token = nextContentExpansionTokenRef.current + 1
    nextContentExpansionTokenRef.current = token
    expansionTokens.add(token)
    return token
  }, [libraryStopScroll, updateFollowIntent])

  const endContentExpansion = useCallback((token: number | null) => {
    const expansionTokens = contentExpansionTokensRef.current
    if (token === null || !expansionTokens?.delete(token)) return false
    if (expansionTokens.size > 0) return false
    contentExpansionTokensRef.current = null
    contentExpansionRestoreFrameRef.current = requestAnimationFrame(() => {
      contentExpansionRestoreFrameRef.current = null
      if (contentExpansionTokensRef.current || isFollowingRef.current) return
      updateFollowIntent(true)
      void libraryScrollToBottom({ animation: "instant" })
    })
    return true
  }, [libraryScrollToBottom, updateFollowIntent])

  const compensateContentAnchor = useCallback((delta: number) => {
    const viewport = scrollRef.current
    if (!viewport || !Number.isFinite(delta) || delta === 0) return false
    contentAnchorCompensationActiveRef.current = true
    viewport.scrollTop += delta
    if (contentAnchorCompensationFrameRef.current !== null) {
      cancelAnimationFrame(contentAnchorCompensationFrameRef.current)
    }
    contentAnchorCompensationFrameRef.current = requestAnimationFrame(() => {
      contentAnchorCompensationFrameRef.current = null
      contentAnchorCompensationActiveRef.current = false
    })
    return true
  }, [scrollRef])

  const contentExpansionController = useMemo<ChatContainerContentExpansionController>(
    () => ({
      beginContentExpansion,
      endContentExpansion,
      scrollRef,
      compensateContentAnchor,
    }),
    [beginContentExpansion, compensateContentAnchor, endContentExpansion, scrollRef],
  )

  const exposedContext = useMemo<ChatContainerContext>(() => ({
    contentRef: stickContext.contentRef,
    scrollRef: stickContext.scrollRef,
    scrollToBottom,
    stopScroll,
    beginContentExpansion,
    endContentExpansion,
    compensateContentAnchor,
    isAtBottom: isFollowing,
    escapedFromLock: !isFollowing,
    state: stickContext.state,
    get targetScrollTop() {
      return stickContext.targetScrollTop
    },
    set targetScrollTop(targetScrollTop) {
      stickContext.targetScrollTop = targetScrollTop
    },
  }), [
    beginContentExpansion,
    compensateContentAnchor,
    endContentExpansion,
    isFollowing,
    scrollToBottom,
    stickContext,
    stopScroll,
  ])

  useImperativeHandle(contextRef, () => exposedContext, [exposedContext])

  useEffect(() => {
    onAtBottomChange?.(isFollowing)
  }, [isFollowing, onAtBottomChange])

  const scheduleFollowRecovery = useCallback(() => {
    if (!isFollowingRef.current || recoveryTimerRef.current !== null) return
    recoveryTimerRef.current = window.setTimeout(() => {
      recoveryTimerRef.current = null
      recoveryFrameRef.current = requestAnimationFrame(() => {
        const startedAt = performance.now()
        recoveryFrameRef.current = null
        if (!isFollowingRef.current) return
        const viewport = scrollRef.current as HTMLDivElement | null
        const beforeScrollTop = viewport?.scrollTop ?? null
        const shouldRecover = Boolean(
          viewport &&
          (!isChatContainerViewportAtBottom(viewport) ||
            !stickContext.state.isAtBottom)
        )
        if (shouldRecover) {
          void libraryScrollToBottom({ animation: "instant" })
        }
        if (isAcpStreamingDiagnosticsEnabled()) {
          const durationMs = performance.now() - startedAt
          const diagnostic = followDiagnosticRef.current
          diagnostic.checkCount += 1
          diagnostic.callbackDurationMs += durationMs
          diagnostic.longestCallbackMs = Math.max(
            diagnostic.longestCallbackMs,
            durationMs,
          )
          if (
            shouldRecover
            && viewport
            && beforeScrollTop !== viewport.scrollTop
          ) diagnostic.scrollWriteCount += 1
          const now = performance.now()
          if (
            lastFollowDiagnosticAtRef.current === 0
            || now - lastFollowDiagnosticAtRef.current >= CHAT_CONTAINER_DIAGNOSTIC_SAMPLE_MS
          ) {
            recordAcpStreamingDiagnostic("chat-follow-sample", () => ({
              sampleDurationMs: lastFollowDiagnosticAtRef.current === 0
                ? null
                : Math.round((now - lastFollowDiagnosticAtRef.current) * 10) / 10,
              checkCount: diagnostic.checkCount,
              scrollWriteCount: diagnostic.scrollWriteCount,
              callbackDurationMs: Math.round(diagnostic.callbackDurationMs * 10) / 10,
              longestCallbackMs: Math.round(diagnostic.longestCallbackMs * 10) / 10,
              following: isFollowingRef.current,
            }))
            lastFollowDiagnosticAtRef.current = now
            diagnostic.checkCount = 0
            diagnostic.scrollWriteCount = 0
            diagnostic.callbackDurationMs = 0
            diagnostic.longestCallbackMs = 0
          }
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
      if (contentAnchorCompensationActiveRef.current) return
      if (
        contentExpansionTokensRef.current &&
        pointerScrollingRef.current &&
        previousScrollTop !== null &&
        currentScrollTop !== previousScrollTop
      ) {
        stopScroll()
      } else if (
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
        cancelContentExpansionRestore()
        updateFollowIntent(true)
      }
      onViewportScroll?.(viewport)
      scheduleFollowRecovery()
    }
    const handleWheel = (event: WheelEvent) => {
      if (event.deltaX !== 0 || event.deltaY !== 0) onViewportUserScroll?.(viewport)
      if (contentExpansionTokensRef.current && event.deltaY !== 0) {
        stopScroll()
      } else if (
        event.deltaY < 0 &&
        viewport.scrollHeight > viewport.clientHeight
      ) {
        stopScroll()
      }
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (CHAT_CONTAINER_SCROLL_KEYS.has(event.key)) onViewportUserScroll?.(viewport)
      const scrollsUp = CHAT_CONTAINER_SCROLL_UP_KEYS.has(event.key)
        || (event.key === " " && event.shiftKey)
      if (
        contentExpansionTokensRef.current &&
        CHAT_CONTAINER_SCROLL_KEYS.has(event.key)
      ) {
        stopScroll()
      } else if (scrollsUp && viewport.scrollHeight > viewport.clientHeight) {
        stopScroll()
      }
    }
    const handlePointerDown = (event: PointerEvent) => {
      pointerScrollingRef.current = event.target === viewport
      if (pointerScrollingRef.current) onViewportUserScroll?.(viewport)
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
    onViewportUserScroll,
    scheduleFollowRecovery,
    cancelContentExpansionRestore,
    scrollRef,
    stopScroll,
    updateFollowIntent,
  ])

  useEffect(() => () => {
    if (contentAnchorCompensationFrameRef.current !== null) {
      cancelAnimationFrame(contentAnchorCompensationFrameRef.current)
    }
  }, [])

  useEffect(() => {
    const content = contentRef.current
    if (!content) return
    let previousHeight: number | null = null
    const observer = new ResizeObserver(([entry]) => {
      const startedAt = performance.now()
      const height = entry?.contentRect.height ?? content.getBoundingClientRect().height
      const heightDelta = previousHeight === null ? 0 : height - previousHeight
      previousHeight = height
      const viewport = scrollRef.current as HTMLDivElement | null
      if (
        contentExpansionTokensRef.current &&
        !isFollowingRef.current &&
        viewport &&
        isChatContainerViewportAtBottom(viewport)
      ) {
        cancelContentExpansionRestore()
        updateFollowIntent(true)
        void libraryScrollToBottom({ animation: "instant" })
      } else {
        scheduleFollowRecovery()
      }
      if (isAcpStreamingDiagnosticsEnabled()) {
        const durationMs = performance.now() - startedAt
        const diagnostic = layoutDiagnosticRef.current
        diagnostic.callbackCount += 1
        diagnostic.durationMs += durationMs
        diagnostic.heightDelta += heightDelta
        diagnostic.longestCallbackMs = Math.max(
          diagnostic.longestCallbackMs,
          durationMs,
        )
        diagnostic.maxHeightDelta = Math.max(
          diagnostic.maxHeightDelta,
          Math.abs(heightDelta),
        )
        const now = performance.now()
        if (
          lastLayoutDiagnosticAtRef.current === 0
          || now - lastLayoutDiagnosticAtRef.current >= CHAT_CONTAINER_DIAGNOSTIC_SAMPLE_MS
        ) {
          recordAcpStreamingDiagnostic("chat-layout-sample", () => ({
            sampleDurationMs: lastLayoutDiagnosticAtRef.current === 0
              ? null
              : Math.round((now - lastLayoutDiagnosticAtRef.current) * 10) / 10,
            callbackCount: diagnostic.callbackCount,
            heightDelta: Math.round(diagnostic.heightDelta * 10) / 10,
            maxHeightDelta: Math.round(diagnostic.maxHeightDelta * 10) / 10,
            callbackDurationMs: Math.round(diagnostic.durationMs * 10) / 10,
            longestCallbackMs: Math.round(diagnostic.longestCallbackMs * 10) / 10,
            following: isFollowingRef.current,
            atBottom: viewport ? isChatContainerViewportAtBottom(viewport) : null,
          }))
          lastLayoutDiagnosticAtRef.current = now
          diagnostic.callbackCount = 0
          diagnostic.heightDelta = 0
          diagnostic.maxHeightDelta = 0
          diagnostic.durationMs = 0
          diagnostic.longestCallbackMs = 0
        }
      }
    })
    observer.observe(content)
    return () => observer.disconnect()
  }, [
    cancelContentExpansionRestore,
    contentRef,
    libraryScrollToBottom,
    scheduleFollowRecovery,
    scrollRef,
    updateFollowIntent,
  ])

  useEffect(() => () => {
    if (recoveryTimerRef.current !== null) {
      window.clearTimeout(recoveryTimerRef.current)
    }
    if (recoveryFrameRef.current !== null) {
      cancelAnimationFrame(recoveryFrameRef.current)
    }
    if (contentExpansionRestoreFrameRef.current !== null) {
      cancelAnimationFrame(contentExpansionRestoreFrameRef.current)
    }
  }, [])

  return (
    <ChatContainerContentExpansionContext.Provider
      value={contentExpansionController}
    >
      {children}
    </ChatContainerContentExpansionContext.Provider>
  )
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
