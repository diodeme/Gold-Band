import { useState, useMemo, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Markdown } from "@/components/prompt-kit/markdown";
import { cn } from "@/lib/utils";
import { Ban, Check, ChevronLeft, ChevronRight, Pencil } from "lucide-react";

export interface ElicitationSchema {
  type: "object";
  properties?: Record<string, ElicitationPropertySchema>;
  required?: string[];
}

export interface ElicitationPropertySchema {
  type: "string" | "array";
  title?: string;
  description?: string;
  oneOf?: Array<{ const: string; title: string }>;
  anyOf?: Array<{ const: string; title: string }>;
  items?: {
    anyOf?: Array<{ const: string; title: string }>;
  };
}

export interface ElicitationCardProps {
  elicitationId: string;
  message: string;
  schema: ElicitationSchema;
  onRespond?: (content: Record<string, unknown>) => void;
  onDecline?: () => void;
}

export interface ElicitationField {
  key: string;
  isSelect: boolean;
  isMulti: boolean;
  isCustom: boolean;
  title?: string;
  description?: string;
  options?: Array<{ value: string; label: string }>;
  hasCustomVariant: boolean;
  customVariantKey?: string;
  customVariantDescription?: string;
}

export interface ElicitationFieldDraft {
  selectedValue: string | null;
  multiValues: string[];
  customText: string;
  customActive: boolean;
}

const ELICITATION_OPTION_INTERACTION_CLASS =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/70";
const ELICITATION_SELECTED_OPTION_CLASS =
  "border-accent-foreground/55 bg-accent text-accent-foreground shadow-[inset_3px_0_0_var(--accent-foreground)]";
const ELICITATION_SELECTED_MARK_CLASS =
  "border-accent-foreground bg-accent-foreground text-background";

export function elicitationOptions(
  prop: ElicitationPropertySchema,
): Array<{ value: string; label: string }> | undefined {
  if (prop.oneOf?.length) {
    return prop.oneOf.map((option) => ({
      value: option.const,
      label: option.title,
    }));
  }
  const multiOptions = prop.anyOf ?? prop.items?.anyOf;
  if (multiOptions?.length) {
    return multiOptions.map((option) => ({
      value: option.const,
      label: option.title,
    }));
  }
  return undefined;
}

/** 从 schema/message 中提取当前步骤对应的单条问题文本 */
export function elicitationQuestionText(
  message: string,
  fieldDescription: string | undefined,
  index: number,
  fallback: string,
): string {
  const description = fieldDescription?.trim();
  if (description) return description;

  // 尝试从 message 中按换行拆分，匹配当前步骤
  const trimmedMessage = message.trim();
  const lines = trimmedMessage.split("\n").map((l) => l.trim()).filter(Boolean);
  if (lines.length > 0) {
    if (lines[index] && !isGenericElicitationMessage(lines[index])) return lines[index];
    if (lines.length === 1 && !isGenericElicitationMessage(lines[0])) return lines[0];
  }
  if (trimmedMessage && !isGenericElicitationMessage(trimmedMessage)) return trimmedMessage;
  return fallback;
}

function isGenericElicitationMessage(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return (
    normalized === "please answer the following questions." ||
    normalized === "please answer the following questions"
  );
}

function shouldShowFieldTitle(
  title: string | undefined,
  questionText: string,
): boolean {
  const trimmedTitle = title?.trim();
  if (!trimmedTitle) return false;
  return trimmedTitle !== questionText.trim();
}

export function elicitationFieldDraft(
  answers: Record<string, unknown>,
  field: ElicitationField,
): ElicitationFieldDraft {
  const emptyDraft: ElicitationFieldDraft = {
    selectedValue: null,
    multiValues: [],
    customText: "",
    customActive: false,
  };
  const customValue = field.customVariantKey
    ? answers[field.customVariantKey]
    : undefined;

  if (typeof customValue === "string" && customValue.trim()) {
    return {
      ...emptyDraft,
      customText: customValue,
      customActive: true,
    };
  }

  const value = answers[field.key];
  if (field.isMulti && Array.isArray(value)) {
    return {
      ...emptyDraft,
      multiValues: value.filter(
        (item): item is string => typeof item === "string",
      ),
    };
  }
  if (field.isSelect && typeof value === "string") {
    return { ...emptyDraft, selectedValue: value };
  }
  if (field.isCustom && typeof value === "string" && value.trim()) {
    return {
      ...emptyDraft,
      customText: value,
      customActive: true,
    };
  }
  return emptyDraft;
}

export function clearElicitationFieldAnswer(
  answers: Record<string, unknown>,
  field: ElicitationField,
): Record<string, unknown> {
  const nextAnswers = { ...answers };
  delete nextAnswers[field.key];
  if (field.customVariantKey) {
    delete nextAnswers[field.customVariantKey];
  }
  return nextAnswers;
}

export function replaceElicitationFieldAnswer(
  answers: Record<string, unknown>,
  field: ElicitationField,
  value: unknown,
  fieldKey?: string,
): Record<string, unknown> {
  const nextAnswers = clearElicitationFieldAnswer(answers, field);
  const answerKey = fieldKey ?? field.key;
  nextAnswers[answerKey] = value;
  if (answerKey !== field.key) {
    nextAnswers[field.key] = value;
  }
  return nextAnswers;
}

export function buildElicitationContent(
  fields: ElicitationField[],
  answers: Record<string, unknown>,
): Record<string, unknown> {
  const content: Record<string, unknown> = {};
  for (const field of fields) {
    const value = answers[field.key];
    if (
      value !== undefined &&
      value !== null &&
      !(typeof value === "string" && value.trim() === "") &&
      !(Array.isArray(value) && value.length === 0)
    ) {
      content[field.key] = value;
    }
    if (field.customVariantKey) {
      const customValue = answers[field.customVariantKey];
      if (typeof customValue === "string" && customValue.trim()) {
        content[field.customVariantKey] = customValue;
      }
    }
  }
  return content;
}

export function ElicitationCard({
  elicitationId,
  message,
  schema,
  onRespond,
  onDecline,
}: ElicitationCardProps) {
  const { t } = useTranslation();

  // ── 向导状态 ──
  const [currentStep, setCurrentStep] = useState(0);
  const [answers, setAnswers] = useState<Record<string, unknown>>({});

  // ── 当前步骤的选择状态 ──
  const [selectedValue, setSelectedValue] = useState<string | null>(null);
  const [multiValues, setMultiValues] = useState<string[]>([]);
  const [customText, setCustomText] = useState("");
  const [customActive, setCustomActive] = useState(false);

  const fields = useMemo(() => {
    if (!schema.properties) return [];
    const entries = Object.entries(schema.properties);
    const selA: Array<{key: string; prop: any; isMulti: boolean; customKey?: string; customSchema?: any}> = [];
    const unmat: Array<[string, any]> = [];
    const claimed = new Set<string>();
    for (const [k, p] of entries) {
      const options = elicitationOptions(p);
      if (options && options.length > 0) {
        const ck = k + "_custom";
        const ce = entries.find(([x]) => x === ck);
        if (ce) claimed.add(ck);
        selA.push({ key: k, prop: p, isMulti: p.type === "array",
          customKey: ce ? ck : undefined, customSchema: ce ? (ce[1] as any) : undefined });
      }
    }
    for (const [k, p] of entries) {
      const options = elicitationOptions(p);
      if (options && options.length > 0) continue;
      if (claimed.has(k)) continue;
      unmat.push([k, p]);
    }
    for (const [ck, cs] of unmat) {
      if (selA.length === 0) break;
      const t = selA.find((s) => !s.customKey) || selA[0];
      if (!t.customKey) { t.customKey = ck; t.customSchema = cs; }
    }
    const result: ElicitationField[] = [];
    for (const s of selA) {
      result.push({
        key: s.key, isSelect: true, isMulti: s.isMulti, isCustom: false,
        title: s.prop.title, description: s.prop.description,
        options: elicitationOptions(s.prop),
        hasCustomVariant: !!s.customKey, customVariantKey: s.customKey,
        customVariantDescription: s.customSchema?.description || s.customSchema?.title,
      });
    }
    if (selA.length === 0) {
      for (const [k, p] of unmat) {
        result.push({ key: k, isSelect: false, isMulti: false, isCustom: true,
          title: p.title, description: p.description, hasCustomVariant: false });
      }
    }
    return result;
  }, [schema]);


  const isMultiStep = fields.length > 1;
  const currentField = fields[currentStep];
  const isLastStep = currentStep === fields.length - 1;

  // schema.required 决定哪些字段是可跳过的
  const requiredKeys = useMemo(
    () => new Set(schema.required ?? []),
    [schema.required],
  );
  const currentIsRequired =
    currentField && requiredKeys.has(currentField.key);

  // 当前页只维护未确认草稿；切换步骤时从已确认答案恢复显示状态。
  useEffect(() => {
    if (!currentField) return;
    const draft = elicitationFieldDraft(answers, currentField);
    setSelectedValue(draft.selectedValue);
    setMultiValues(draft.multiValues);
    setCustomText(draft.customText);
    setCustomActive(draft.customActive);
  }, [answers, currentField, currentStep]);

  // elicitationId 变化时完全重置（key prop 已保证重新挂载，此处是兜底）
  useEffect(() => {
    setCurrentStep(0);
    setAnswers({});
    setSelectedValue(null);
    setMultiValues([]);
    setCustomText("");
    setCustomActive(false);
  }, [elicitationId]);

  // 步骤提交：保存答案 → 下一步或最终提交
  const handleStepSubmit = useCallback(
    (value: unknown, fieldKey?: string) => {
      if (!currentField) return;
      const nextAnswers = replaceElicitationFieldAnswer(
        answers,
        currentField,
        value,
        fieldKey,
      );
      if (isLastStep) {
        onRespond?.(buildElicitationContent(fields, nextAnswers));
      } else {
        setAnswers(nextAnswers);
        setCurrentStep((prev) => prev + 1);
      }
    },
    [answers, currentField, fields, isLastStep, onRespond],
  );

  // 跳过当前步骤
  const handleSkip = useCallback(() => {
    if (!currentField) return;
    const nextAnswers = clearElicitationFieldAnswer(answers, currentField);
    if (isLastStep) {
      onRespond?.(buildElicitationContent(fields, nextAnswers));
    } else {
      setAnswers(nextAnswers);
      setCurrentStep((prev) => prev + 1);
    }
  }, [answers, currentField, fields, isLastStep, onRespond]);

  // 回退到上一步
  const handleBack = useCallback(() => {
    if (currentStep > 0) {
      setCurrentStep((prev) => prev - 1);
    }
  }, [currentStep]);

  if (!currentField) {
    return null;
  }

  const actionLabel = isLastStep
    ? t("acp.elicitation.submit", "提交")
    : t("acp.elicitation.next", "下一步");
  const questionText = elicitationQuestionText(
    message,
    currentField.description,
    currentStep,
    t("acp.elicitation.questionFallback", "请选择一个答案"),
  );
  const showFieldTitle = shouldShowFieldTitle(currentField.title, questionText);

  // ── 进度指示器 ──
  const ProgressDots = isMultiStep ? (
    <div className="flex items-center justify-center gap-1.5 mb-0.5">
      {fields.map((_, i) => (
        <span
          key={i}
          className={cn(
            "size-1.5 rounded-full transition-colors",
            i < currentStep
              ? "bg-foreground/40"
              : i === currentStep
                ? "bg-foreground"
                : "bg-muted-foreground/20",
          )}
        />
      ))}
      <span className="ml-1 text-[10px] text-muted-foreground">
        {t("acp.elicitation.step", { current: currentStep + 1, total: fields.length })}
      </span>
    </div>
  ) : null;

  // ── 回退按钮 ──
  const BackButton =
    isMultiStep && currentStep > 0 ? (
      <button
        type="button"
        onClick={handleBack}
        className={cn(
          "inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors",
        )}
      >
        <ChevronLeft className="size-3" />
        {t("acp.elicitation.back", "返回")}
      </button>
    ) : null;

  return (
    <Card className="my-2 gap-0 border-border/70 py-0">
      <CardContent className="space-y-2.5 px-5 py-4">
        {/* 进度指示器 */}
        {ProgressDots}

        {/* 当前字段标题 */}
        {showFieldTitle && (
          <div className="text-[13px] font-medium leading-5 text-foreground">
            {currentField.title}
          </div>
        )}

        {/* 当前问题文本 */}
        {questionText && (
          <div className="text-[13px] leading-6 text-muted-foreground">
            <Markdown>{questionText}</Markdown>
          </div>
        )}

        {/* 单选：选中态 + 确认按钮 */}
        {currentField.isSelect && !currentField.isMulti && (
          <div className="space-y-1">
            {currentField.options!.map((o) => {
              const sel = !customActive && selectedValue === o.value;
              return (<button key={o.value} type="button"
                  aria-pressed={sel}
                  onClick={() => { setSelectedValue(o.value); setCustomActive(false); setCustomText(""); }}
                  className={cn("w-full flex items-center justify-between text-left rounded-md border px-3 py-2 text-[13px] leading-5 transition-all",
                    sel ? ELICITATION_SELECTED_OPTION_CLASS : "hover:border-accent-foreground/35 hover:bg-accent/60",
                    ELICITATION_OPTION_INTERACTION_CLASS,
                    "active:scale-[0.995]", "disabled:opacity-50 disabled:cursor-not-allowed")}
                ><span className="font-medium">{o.label}</span>
                  {sel ? (
                    <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full border", ELICITATION_SELECTED_MARK_CLASS)}>
                      <Check className="size-3.5" strokeWidth={3} />
                    </span>
                  ) : <ChevronRight className="size-4 opacity-0 transition-opacity group-hover:opacity-50 text-muted-foreground" />}</button>);
            })}
            {currentField.hasCustomVariant && !customActive && (
              <button type="button" onClick={() => { setCustomActive(true); setSelectedValue(null); }}
                className={cn("w-full flex items-center gap-2 rounded-md border border-dashed px-3 py-2 text-[13px] text-muted-foreground transition-colors",
                  "hover:border-accent-foreground/40 hover:text-foreground",
                  ELICITATION_OPTION_INTERACTION_CLASS)}
              ><Pencil className="size-4" /><span>{t("acp.elicitation.customPlaceholder", "其他答案...")}</span></button>
            )}
            {currentField.hasCustomVariant && customActive && (
              <div className="space-y-1.5">
                <button type="button" onClick={() => { setCustomActive(false); setCustomText(""); }}
                  className={cn("text-xs text-muted-foreground hover:text-foreground transition-colors")}
                >← {t("acp.elicitation.backToOptions", "返回选项")}</button>
                <Input autoFocus value={customText}
                  onChange={(e) => setCustomText(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && customText.trim()) {
                    handleStepSubmit(customText.trim(), currentField.customVariantKey); }}}
                  placeholder={currentField.customVariantDescription || t("acp.elicitation.customPlaceholder", "输入答案后按回车...")}
                  className="flex-1" />
              </div>
            )}
          </div>
        )}

        {/* ── 多选 ── */}
        {currentField.isMulti && (
          <div className="space-y-1">
            {currentField.options!.map((option) => {
              const selected = multiValues.includes(option.value);
              return (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={selected}
                  onClick={() =>
                    setMultiValues((prev) =>
                      selected
                        ? prev.filter((v) => v !== option.value)
                        : [...prev, option.value],
                    )
                  }
                  className={cn(
                    "w-full flex items-center gap-2.5 text-left rounded-md border px-3 py-2 text-[13px] leading-5 transition-all",
                    selected
                      ? ELICITATION_SELECTED_OPTION_CLASS
                      : "hover:border-accent-foreground/35 hover:bg-accent/60",
                    ELICITATION_OPTION_INTERACTION_CLASS,
                    "active:scale-[0.995]",
                    "disabled:opacity-50",
                  )}
                >
                  <span
                    className={cn(
                      "size-4 rounded border-2 flex items-center justify-center shrink-0 transition-colors",
                      selected
                        ? ELICITATION_SELECTED_MARK_CLASS
                        : "border-muted-foreground/30",
                    )}
                  >
                    {selected && (
                      <Check className="size-3" strokeWidth={3} />
                    )}
                  </span>
                  <span>{option.label}</span>
                </button>
              );
            })}
          </div>
        )}

        {/* ── 自定义文本 ── */}
        {currentField.isCustom && !currentField.isSelect && (
          <div>
            {!customActive ? (
              <button
                type="button"
                onClick={() => setCustomActive(true)}
                className={cn(
                  "w-full flex items-center gap-2 rounded-md border border-dashed px-3 py-2 text-[13px] text-muted-foreground transition-colors",
                  "hover:border-accent-foreground/40 hover:text-foreground",
                  ELICITATION_OPTION_INTERACTION_CLASS,
                  "disabled:opacity-50",
                )}
              >
                <Pencil className="size-4" />
                <span>
                  {currentField.title ||
                    t("acp.elicitation.customPlaceholder", "其他答案...")}
                </span>
              </button>
            ) : (
              <div className="flex gap-2">
                <Input
                  autoFocus
                  value={customText}
                  onChange={(e) => setCustomText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && customText.trim()) {
                      handleStepSubmit(customText.trim());
                    }
                  }}
                  placeholder={
                    currentField.description ||
                    t(
                      "acp.elicitation.customPlaceholder",
                      "输入你的答案后按回车...",
                    )
                  }
                  className="flex-1"
                />
              </div>
            )}
        </div>
        )}

        {/* ── 操作按钮区 ── */}
        <div className="flex items-center justify-between pt-0.5">
          <div className="flex items-center gap-2">
            {BackButton}
            {onDecline && (
              <button
                type="button"
                onClick={onDecline}
                className="inline-flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
              >
                <Ban className="size-3" />
                {t("acp.elicitation.skipQuestion", "跳过此问题")}
              </button>
            )}
          </div>
          <div className="ml-auto flex items-center gap-1.5">
            {/* 跳过按钮（非必填字段） */}
            {!currentIsRequired && (
              <button
                type="button"
                onClick={handleSkip}
                className={cn(
                  "px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:text-foreground",
                )}
              >
                {t("acp.elicitation.skip", "跳过")}
              </button>
            )}

            {/* 单选：确认当前选中 */}
            {currentField.isSelect && !currentField.isMulti && (
              <button type="button"
                disabled={customActive ? !customText.trim() : !selectedValue}
                onClick={() => {
                  if (customActive && customText.trim()) {
                    handleStepSubmit(customText.trim(), currentField.customVariantKey);
                  } else if (selectedValue) { handleStepSubmit(selectedValue); }
                }}
                className={cn("inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors",
                  "hover:bg-primary/90", "disabled:opacity-50 disabled:cursor-not-allowed")}
              >{actionLabel}<ChevronRight className="size-3" /></button>
            )}
            {/* 多选：确认按钮 */}
            {currentField.isMulti && (
              <button type="button"
                disabled={customActive ? !customText.trim() : multiValues.length === 0}
                onClick={() => {
                  if (customActive && customText.trim()) {
                    handleStepSubmit(customText.trim(), currentField.customVariantKey);
                  } else { handleStepSubmit(multiValues); }
                }}
                className={cn("inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors",
                  "hover:bg-primary/90", "disabled:opacity-50 disabled:cursor-not-allowed")}
              >{actionLabel}<ChevronRight className="size-3" /></button>
            )}
            {/* 自定义文本：确认按钮 */}
            {currentField.isCustom && !currentField.isSelect && customActive && (
              <button type="button" disabled={!customText.trim()}
                onClick={() => handleStepSubmit(customText.trim())}
                className={cn("shrink-0 inline-flex items-center justify-center size-8 rounded-md bg-primary text-primary-foreground transition-colors",
                  "hover:bg-primary/90", "disabled:opacity-50 disabled:cursor-not-allowed")}
              ><ChevronRight className="size-4" /></button>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
