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
  oneOf?: ElicitationEnumOption[];
  anyOf?: ElicitationEnumOption[];
  items?: {
    anyOf?: ElicitationEnumOption[];
  };
  _meta?: Record<string, unknown>;
}

export interface ElicitationEnumOption {
  const: string;
  title: string;
  description?: string;
  _meta?: Record<string, unknown>;
}

export interface ElicitationOption {
  value: string;
  label: string;
  description?: string;
  preview?: string;
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
  options?: ElicitationOption[];
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
): ElicitationOption[] | undefined {
  const normalizeOption = (option: ElicitationEnumOption): ElicitationOption => {
    const preview = elicitationOptionPreview(option._meta);
    return {
      value: option.const,
      label: option.title,
      ...(option.description ? { description: option.description } : {}),
      ...(preview ? { preview } : {}),
    };
  };
  if (prop.oneOf?.length) {
    return prop.oneOf.map(normalizeOption);
  }
  const multiOptions = prop.anyOf ?? prop.items?.anyOf;
  if (multiOptions?.length) {
    return multiOptions.map(normalizeOption);
  }
  return undefined;
}

function elicitationOptionPreview(
  meta: Record<string, unknown> | undefined,
): string | undefined {
  const optionMeta = meta?.["_claude/askUserQuestionOption"];
  if (!optionMeta || typeof optionMeta !== "object" || Array.isArray(optionMeta)) {
    return undefined;
  }
  const preview = (optionMeta as Record<string, unknown>).preview;
  return typeof preview === "string" ? preview : undefined;
}

export function elicitationFormMessage(message: string): string | undefined {
  const trimmedMessage = message.trim();
  if (!trimmedMessage || isGenericElicitationMessage(trimmedMessage)) {
    return undefined;
  }
  return trimmedMessage;
}

/** 兼容调用方：字段说明优先，否则返回完整表单 message，绝不按行拆题。 */
export function elicitationQuestionText(
  message: string,
  fieldDescription: string | undefined,
  _index: number,
  fallback: string,
): string {
  const description = fieldDescription?.trim();
  if (description) return description;
  return elicitationFormMessage(message) ?? fallback;
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

function customAnswerTarget(
  prop: ElicitationPropertySchema,
): string | undefined {
  const marker = prop._meta?.["_askUserQuestionCustomAnswer"];
  if (!marker || typeof marker !== "object" || Array.isArray(marker)) {
    return undefined;
  }
  const values = marker as Record<string, unknown>;
  return values.isCustomAnswer === true && typeof values.questionId === "string"
    ? values.questionId
    : undefined;
}

export function normalizeElicitationFields(
  schema: ElicitationSchema,
): ElicitationField[] {
  const entries = Object.entries(schema.properties ?? {});
  const properties = new Map(entries);
  const selectEntries = entries.filter(([, prop]) => {
    const options = elicitationOptions(prop);
    return Boolean(options?.length);
  });
  const selectKeys = new Set(selectEntries.map(([key]) => key));
  const customByQuestion = new Map<
    string,
    { key: string; schema: ElicitationPropertySchema }
  >();
  const claimedCustomKeys = new Set<string>();

  // Latest ACP shape: explicit metadata wins over naming conventions.
  for (const [key, prop] of entries) {
    const target = customAnswerTarget(prop);
    if (
      target &&
      prop.type === "string" &&
      selectKeys.has(target) &&
      !customByQuestion.has(target)
    ) {
      customByQuestion.set(target, { key, schema: prop });
      claimedCustomKeys.add(key);
    }
  }

  // Claude Agent ACP 0.45.1+: question_n_custom pairs with question_n.
  for (const [key] of selectEntries) {
    if (customByQuestion.has(key)) continue;
    const customKey = `${key}_custom`;
    const customSchema = properties.get(customKey);
    if (
      customSchema?.type === "string" &&
      !elicitationOptions(customSchema)?.length
    ) {
      customByQuestion.set(key, { key: customKey, schema: customSchema });
      claimedCustomKeys.add(customKey);
    }
  }

  const fields: ElicitationField[] = selectEntries.map(([key, prop]) => {
    const custom = customByQuestion.get(key);
    return {
      key,
      isSelect: true,
      isMulti: prop.type === "array",
      isCustom: false,
      title: prop.title,
      description: prop.description,
      options: elicitationOptions(prop),
      hasCustomVariant: Boolean(custom),
      customVariantKey: custom?.key,
      customVariantDescription:
        custom?.schema.description ?? custom?.schema.title,
    };
  });

  // Legacy global customAnswer and ordinary text fields remain independent.
  // They must not be guessed as companions for an unrelated select field.
  for (const [key, prop] of entries) {
    if (elicitationOptions(prop)?.length || claimedCustomKeys.has(key)) continue;
    fields.push({
      key,
      isSelect: false,
      isMulti: false,
      isCustom: true,
      title: prop.title,
      description: prop.description,
      hasCustomVariant: false,
    });
  }

  return fields;
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

  const fields = useMemo(() => normalizeElicitationFields(schema), [schema]);


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
  const formMessage = elicitationFormMessage(message);
  const questionText =
    currentField.description?.trim() ||
    (!formMessage
      ? t("acp.elicitation.questionFallback", "请选择一个答案")
      : "");
  const showFieldTitle = shouldShowFieldTitle(
    currentField.title,
    questionText || formMessage || "",
  );

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

        {/* 表单级 message：完整保留，不能按换行拆成步骤。 */}
        {formMessage && (
          <div className="text-[13px] leading-6 text-muted-foreground">
            <Markdown>{formMessage}</Markdown>
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
                ><div className="min-w-0 flex-1">
                    <div className="font-medium">{o.label}</div>
                    {o.description && (
                      <div className="mt-0.5 text-xs font-normal text-muted-foreground">
                        {o.description}
                      </div>
                    )}
                    {sel && o.preview && (
                      <div className="mt-2 border-t border-border/60 pt-2 text-xs font-normal text-muted-foreground">
                        <Markdown>{o.preview}</Markdown>
                      </div>
                    )}
                  </div>
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
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{option.label}</div>
                    {option.description && (
                      <div className="mt-0.5 text-xs font-normal text-muted-foreground">
                        {option.description}
                      </div>
                    )}
                    {selected && option.preview && (
                      <div className="mt-2 border-t border-border/60 pt-2 text-xs font-normal text-muted-foreground">
                        <Markdown>{option.preview}</Markdown>
                      </div>
                    )}
                  </div>
                </button>
              );
            })}
            {currentField.hasCustomVariant && !customActive && (
              <button
                type="button"
                onClick={() => {
                  setCustomActive(true);
                  setMultiValues([]);
                }}
                className={cn(
                  "w-full flex items-center gap-2 rounded-md border border-dashed px-3 py-2 text-[13px] text-muted-foreground transition-colors",
                  "hover:border-accent-foreground/40 hover:text-foreground",
                  ELICITATION_OPTION_INTERACTION_CLASS,
                )}
              >
                <Pencil className="size-4" />
                <span>{t("acp.elicitation.customPlaceholder", "其他答案...")}</span>
              </button>
            )}
            {currentField.hasCustomVariant && customActive && (
              <div className="space-y-1.5">
                <button
                  type="button"
                  onClick={() => {
                    setCustomActive(false);
                    setCustomText("");
                  }}
                  className="text-xs text-muted-foreground transition-colors hover:text-foreground"
                >
                  ← {t("acp.elicitation.backToOptions", "返回选项")}
                </button>
                <Input
                  autoFocus
                  value={customText}
                  onChange={(event) => setCustomText(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && customText.trim()) {
                      handleStepSubmit(
                        customText.trim(),
                        currentField.customVariantKey,
                      );
                    }
                  }}
                  placeholder={
                    currentField.customVariantDescription ||
                    t("acp.elicitation.customPlaceholder", "输入答案后按回车...")
                  }
                />
              </div>
            )}
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
