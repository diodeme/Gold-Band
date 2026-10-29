"use client"

import { Textarea } from "@/components/ui/textarea"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { COMPOSER_LEADING_ADORNMENT_SLOT_CLASS_NAME } from "@/lib/conversation-composer-layout"
import { useLeadingAdornmentTextIndent } from "@/hooks/useLeadingAdornmentTextIndent"
import React, {
  useCallback,
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
} from "react"

type PromptInputContextType = {
  isLoading: boolean
  value: string
  setValue: (value: string) => void
  maxHeight: number | string
  onSubmit?: () => void
  disabled?: boolean
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
}

const PromptInputContext = createContext<PromptInputContextType>({
  isLoading: false,
  value: "",
  setValue: () => {},
  maxHeight: 240,
  onSubmit: undefined,
  disabled: false,
  textareaRef: React.createRef<HTMLTextAreaElement>(),
})

function usePromptInput() {
  return useContext(PromptInputContext)
}

export function promptInputTextareaSize(
  scrollHeight: number,
  maxHeight: number | string,
): { height: string; overflowY: "auto" | "hidden" } {
  if (typeof maxHeight === "number") {
    return {
      height: `${Math.min(scrollHeight, maxHeight)}px`,
      overflowY: scrollHeight > maxHeight ? "auto" : "hidden",
    }
  }

  return {
    height: `min(${scrollHeight}px, ${maxHeight})`,
    overflowY: "auto",
  }
}

const PROMPT_INPUT_INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "textarea",
  "select",
  '[role="button"]',
  '[role="combobox"]',
  '[role="menuitem"]',
  '[role="menuitemcheckbox"]',
  '[role="menuitemradio"]',
  '[contenteditable="true"]',
  "[data-prompt-input-interactive]",
].join(",")

export function shouldFocusPromptInputTextarea(target: unknown) {
  if (!target || typeof target !== "object") return true

  const closest = (target as { closest?: (selector: string) => unknown }).closest
  if (typeof closest !== "function") return true

  return !closest.call(target, PROMPT_INPUT_INTERACTIVE_SELECTOR)
}

export type PromptInputProps = {
  isLoading?: boolean
  value?: string
  onValueChange?: (value: string) => void
  maxHeight?: number | string
  onSubmit?: () => void
  children: React.ReactNode
  className?: string
  disabled?: boolean
} & React.ComponentProps<"div">

function PromptInput({
  className,
  isLoading = false,
  maxHeight = 240,
  value,
  onValueChange,
  onSubmit,
  children,
  disabled = false,
  onClick,
  ...props
}: PromptInputProps) {
  const [internalValue, setInternalValue] = useState(value || "")
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const handleChange = (newValue: string) => {
    setInternalValue(newValue)
    onValueChange?.(newValue)
  }

  const handleClick: React.MouseEventHandler<HTMLDivElement> = (e) => {
    if (!disabled && shouldFocusPromptInputTextarea(e.target)) {
      textareaRef.current?.focus()
    }
    onClick?.(e)
  }

  return (
    <TooltipProvider>
      <PromptInputContext.Provider
        value={{
          isLoading,
          value: value ?? internalValue,
          setValue: onValueChange ?? handleChange,
          maxHeight,
          onSubmit,
          disabled,
          textareaRef,
        }}
      >
        <div
          onClick={handleClick}
          className={cn(
            "border-input bg-background cursor-text rounded-3xl border p-2 shadow-xs",
            disabled && "cursor-not-allowed opacity-60",
            className
          )}
          {...props}
          data-slot="prompt-input"
          data-theme-role="composer"
        >
          {children}
        </div>
      </PromptInputContext.Provider>
    </TooltipProvider>
  )
}

export type PromptInputTextareaProps = {
  disableAutosize?: boolean
  textareaDisabled?: boolean
  valuePrefix?: string
  leadingAdornment?: React.ReactNode
  containerClassName?: string
} & React.ComponentProps<typeof Textarea>

const PROMPT_INPUT_SCROLL_BOTTOM_TOLERANCE_PX = 2

const PROMPT_INPUT_MEASUREMENT_STYLE: React.CSSProperties = {
  position: "absolute",
  top: 0,
  left: 0,
  height: 0,
  minHeight: 0,
  maxHeight: "none",
  overflow: "hidden",
  visibility: "hidden",
  pointerEvents: "none",
}

function PromptInputTextarea({
  className,
  ref: externalRef,
  onKeyDown,
  onScroll,
  disableAutosize = false,
  textareaDisabled,
  valuePrefix = "",
  leadingAdornment,
  containerClassName,
  style,
  ...props
}: PromptInputTextareaProps) {
  const { value, setValue, maxHeight, onSubmit, disabled, textareaRef } =
    usePromptInput()
  const effectiveDisabled = textareaDisabled ?? disabled
  const effectiveValuePrefix = valuePrefix && value.startsWith(valuePrefix) ? valuePrefix : ""
  const textareaValue = effectiveValuePrefix ? value.slice(effectiveValuePrefix.length) : value
  const hasLeadingAdornment = Boolean(leadingAdornment && effectiveValuePrefix)
  const leadingAdornmentLayout = useLeadingAdornmentTextIndent(hasLeadingAdornment)
  const measurementRef = useRef<HTMLTextAreaElement>(null)
  const scrollStateRef = useRef({ scrollTop: 0, atBottom: true })

  const rememberScrollState = useCallback((el: HTMLTextAreaElement) => {
    scrollStateRef.current = {
      scrollTop: el.scrollTop,
      atBottom: el.scrollHeight - el.scrollTop - el.clientHeight
        <= PROMPT_INPUT_SCROLL_BOTTOM_TOLERANCE_PX,
    }
  }, [])

  const adjustHeight = useCallback((el: HTMLTextAreaElement | null) => {
    const measurement = measurementRef.current
    if (!el || !measurement || disableAutosize) return

    const previousScroll = scrollStateRef.current
    // Never shrink the live textarea for measurement: in uncapped mode that also
    // collapses the ancestor scroll range and clamps its scrollTop to zero.
    measurement.style.width = `${el.getBoundingClientRect().width}px`
    const contentHeight = measurement.scrollHeight
    const size = promptInputTextareaSize(contentHeight, maxHeight)
    el.style.height = size.height
    el.style.overflowY = size.overflowY
    const maxScrollTop = Math.max(0, contentHeight - el.clientHeight)
    if (size.overflowY === "auto") {
      el.scrollTop = previousScroll.atBottom
        ? maxScrollTop
        : Math.min(previousScroll.scrollTop, maxScrollTop)
    }
    scrollStateRef.current = {
      scrollTop: el.scrollTop,
      atBottom: maxScrollTop - el.scrollTop <= PROMPT_INPUT_SCROLL_BOTTOM_TOLERANCE_PX,
    }
  }, [disableAutosize, maxHeight])

  const handleRef = useCallback((el: HTMLTextAreaElement | null) => {
    textareaRef.current = el
    if (typeof externalRef === "function") {
      externalRef(el)
    } else if (externalRef) {
      externalRef.current = el
    }
    if (el) rememberScrollState(el)
  }, [externalRef, rememberScrollState, textareaRef])

  const handleScroll = useCallback((event: React.UIEvent<HTMLTextAreaElement>) => {
    rememberScrollState(event.currentTarget)
    onScroll?.(event)
  }, [onScroll, rememberScrollState])

  useLayoutEffect(() => {
    adjustHeight(textareaRef.current)
  }, [adjustHeight, textareaRef, textareaValue])

  useLayoutEffect(() => {
    const el = textareaRef.current
    if (!el || disableAutosize || typeof ResizeObserver === "undefined") return

    const observer = new ResizeObserver(() => {
      // Height changes also trigger this observer; only a new width needs remeasurement.
      if (measurementRef.current?.style.width !== `${el.getBoundingClientRect().width}px`) {
        adjustHeight(el)
      }
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [adjustHeight, disableAutosize, textareaRef])

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setValue(`${effectiveValuePrefix}${e.target.value}`)
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    onKeyDown?.(e)
    if (e.defaultPrevented) return
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      onSubmit?.()
    }
  }

  const textareaClassName = cn(
    "text-primary min-h-[44px] min-w-0 flex-1 resize-none border-none bg-transparent shadow-none outline-none focus-visible:ring-0 focus-visible:ring-offset-0 dark:bg-transparent",
    className,
    hasLeadingAdornment && "px-0"
  )

  // The wrapper is always rendered so the textarea keeps its DOM identity (focus and caret) when a
  // leading adornment appears or disappears; only the adornment slot and horizontal inset change.
  return (
    <div
      data-slot="prompt-input-textarea-field"
      data-leading-adornment={hasLeadingAdornment ? "true" : undefined}
      className={cn("relative min-w-0", hasLeadingAdornment && "px-2.5", containerClassName)}
    >
      {hasLeadingAdornment ? (
        <span
          ref={leadingAdornmentLayout.adornmentRef}
          className={cn(COMPOSER_LEADING_ADORNMENT_SLOT_CLASS_NAME, "left-2.5 top-2")}
        >
          {leadingAdornment}
        </span>
      ) : null}
      <Textarea
        ref={handleRef}
        value={textareaValue}
        style={{
          ...style,
          ...leadingAdornmentLayout.textareaStyle,
        }}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onScroll={handleScroll}
        className={textareaClassName}
        rows={1}
        disabled={effectiveDisabled}
        {...props}
      />
      {!disableAutosize ? (
        <Textarea
          ref={measurementRef}
          data-slot="prompt-input-measurement"
          aria-hidden="true"
          tabIndex={-1}
          readOnly
          rows={1}
          value={textareaValue || props.placeholder || " "}
          wrap={props.wrap}
          dir={props.dir}
          className={textareaClassName}
          style={{
            ...style,
            ...leadingAdornmentLayout.textareaStyle,
            ...PROMPT_INPUT_MEASUREMENT_STYLE,
          }}
        />
      ) : null}
    </div>
  )
}

export type PromptInputActionsProps = React.HTMLAttributes<HTMLDivElement>

function PromptInputActions({
  children,
  className,
  ...props
}: PromptInputActionsProps) {
  return (
    <div className={cn("flex items-center gap-2", className)} {...props}>
      {children}
    </div>
  )
}

export type PromptInputActionProps = {
  className?: string
  tooltip: React.ReactNode
  children: React.ReactNode
  side?: "top" | "bottom" | "left" | "right"
} & React.HTMLAttributes<HTMLSpanElement>

function PromptInputAction({
  tooltip,
  children,
  className,
  side = "top",
  ...props
}: PromptInputActionProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          data-slot="prompt-input-action"
          className={cn("inline-flex", className)}
          onClick={(event) => event.stopPropagation()}
          {...props}
        >
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent side={side}>{tooltip}</TooltipContent>
    </Tooltip>
  )
}

export {
  PromptInput,
  PromptInputTextarea,
  PromptInputActions,
  PromptInputAction,
}
