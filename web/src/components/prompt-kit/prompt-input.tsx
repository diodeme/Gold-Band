"use client"

import { Textarea } from "@/components/ui/textarea"
import { TooltipProvider } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
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

function PromptInputTextarea({
  className,
  ref: externalRef,
  onKeyDown,
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

  const adjustHeight = useCallback((el: HTMLTextAreaElement | null) => {
    if (!el || disableAutosize) return

    el.style.height = "auto"
    const size = promptInputTextareaSize(el.scrollHeight, maxHeight)
    el.style.height = size.height
    el.style.overflowY = size.overflowY
  }, [disableAutosize, maxHeight])

  const handleRef = useCallback((el: HTMLTextAreaElement | null) => {
    textareaRef.current = el
    if (typeof externalRef === "function") {
      externalRef(el)
    } else if (externalRef) {
      externalRef.current = el
    }
    adjustHeight(el)
  }, [adjustHeight, externalRef, textareaRef])

  useLayoutEffect(() => {
    adjustHeight(textareaRef.current)
  }, [adjustHeight, textareaRef, textareaValue])

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

  const textarea = (
    <Textarea
      ref={handleRef}
      value={textareaValue}
      style={{ ...style, ...leadingAdornmentLayout.textareaStyle }}
      onChange={handleChange}
      onKeyDown={handleKeyDown}
      className={cn(
        "text-primary min-h-[44px] min-w-0 flex-1 resize-none border-none bg-transparent shadow-none outline-none focus-visible:ring-0 focus-visible:ring-offset-0 dark:bg-transparent",
        hasLeadingAdornment && "px-0 py-0",
        className
      )}
      rows={1}
      disabled={effectiveDisabled}
      {...props}
    />
  )

  if (!hasLeadingAdornment) return textarea

  return (
    <div
      data-slot="prompt-input-textarea-with-adornment"
      className={cn("relative min-w-0 px-3 py-2", containerClassName)}
    >
      <span
        ref={leadingAdornmentLayout.adornmentRef}
        className="absolute left-3 top-2 z-10 inline-flex"
      >
        {leadingAdornment}
      </span>
      {textarea}
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
  className: _className,
  side: _side = "top",
  ..._props
}: PromptInputActionProps) {
  const title = promptInputActionTitle(tooltip)

  return (
    <span
      data-slot="prompt-input-action"
      className="inline-flex"
      title={title}
      onClick={(event) => event.stopPropagation()}
    >
      {children}
    </span>
  )
}

function promptInputActionTitle(node: React.ReactNode) {
  if (typeof node === "string" || typeof node === "number") return String(node)
  return undefined
}

export {
  PromptInput,
  PromptInputTextarea,
  PromptInputActions,
  PromptInputAction,
  promptInputActionTitle,
}
