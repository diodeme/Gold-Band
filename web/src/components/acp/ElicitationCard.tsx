import { useState, useMemo, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Markdown } from "@/components/prompt-kit/markdown";
import { cn } from "@/lib/utils";
import { Check, ChevronLeft, ChevronRight, Pencil } from "lucide-react";

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
}

export interface ElicitationCardProps {
  elicitationId: string;
  message: string;
  schema: ElicitationSchema;
  onRespond: (content: Record<string, unknown>) => void;
  /** 已确认的回答内容，设置后卡片变为只读展示态 */
  confirmedContent?: Record<string, unknown> | null;
}

function formatConfirmedChoice(
  schema: ElicitationSchema,
  content: Record<string, unknown>,
): string {
  if (!schema.properties) return JSON.stringify(content);
  const labels: string[] = [];
  for (const [key, prop] of Object.entries(schema.properties)) {
    const val = content[key];
    if (val === undefined || val === null) continue;
    if (prop.oneOf) {
      const match = prop.oneOf.find((o) => o.const === val);
      labels.push(match?.title ?? String(val));
    } else if (prop.anyOf && Array.isArray(val)) {
      const matches = val
        .map(
          (v: unknown) =>
            prop.anyOf!.find((o) => o.const === String(v))?.title ?? String(v),
        )
        .join("、");
      labels.push(matches);
    } else if (typeof val === "string" && val.trim()) {
      labels.push(val.trim());
    }
  }
  return labels.length > 0 ? labels.join("；") : JSON.stringify(content);
}

/** 从 message 中提取当前步骤对应的单条问题文本 */
export function stepMessage(
  message: string,
  fieldTitle: string | undefined,
  index: number,
  fallback: string,
): string {
  const title = fieldTitle?.trim();
  if (title) return title;

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

export function ElicitationCard({
  elicitationId,
  message,
  schema,
  onRespond,
  confirmedContent,
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
      if ((p.oneOf && p.oneOf.length > 0) || (p.anyOf && p.anyOf.length > 0)) {
        const ck = k + "_custom";
        const ce = entries.find(([x]) => x === ck);
        if (ce) claimed.add(ck);
        selA.push({ key: k, prop: p, isMulti: !!(p.anyOf && p.anyOf.length > 0),
          customKey: ce ? ck : undefined, customSchema: ce ? (ce[1] as any) : undefined });
      }
    }
    for (const [k, p] of entries) {
      if ((p.oneOf && p.oneOf.length > 0) || (p.anyOf && p.anyOf.length > 0)) continue;
      if (claimed.has(k)) continue;
      unmat.push([k, p]);
    }
    for (const [ck, cs] of unmat) {
      if (selA.length === 0) break;
      const t = selA.find((s) => !s.customKey) || selA[0];
      if (!t.customKey) { t.customKey = ck; t.customSchema = cs; }
    }
    const result: Array<{
      key: string; isSelect: boolean; isMulti: boolean; isCustom: boolean;
      title?: string; description?: string;
      options?: Array<{ value: string; label: string }>;
      hasCustomVariant: boolean; customVariantKey?: string; customVariantDescription?: string;
    }> = [];
    for (const s of selA) {
      const ho = !!(s.prop.oneOf && s.prop.oneOf.length > 0);
      result.push({
        key: s.key, isSelect: true, isMulti: s.isMulti, isCustom: false,
        title: s.prop.title, description: s.prop.description,
        options: ho ? s.prop.oneOf!.map((o: any) => ({ value: o.const, label: o.title }))
                   : s.prop.anyOf ? s.prop.anyOf.map((o: any) => ({ value: o.const, label: o.title })) : undefined,
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

  // ── 步骤切换时重置选择状态 ──
  useEffect(() => {
    setSelectedValue(null);
    setMultiValues([]);
    setCustomText("");
    setCustomActive(false);
  }, [currentStep]);

  // ── elicitationId 变化时完全重置（key prop 已保证重新挂载，此处是兜底）──
  useEffect(() => {
    setCurrentStep(0);
    setAnswers({});
    setSelectedValue(null);
    setMultiValues([]);
    setCustomText("");
    setCustomActive(false);
  }, [elicitationId]);

  // ── 构建完整 content ──
  const buildContent = useCallback(
    (overrides?: Record<string, unknown>) => {
      const merged = { ...answers, ...overrides };
      const content: Record<string, unknown> = {};
      for (const field of fields) {
        const val = merged[field.key];
        if (val !== undefined && val !== null) {
          if (typeof val === "string" && val.trim() === "") {}
          else if (Array.isArray(val) && val.length === 0) {}
          else { content[field.key as string] = val; }
        }
        if (field.customVariantKey) {
          const cv = merged[field.customVariantKey];
          if (cv !== undefined && cv !== null && typeof cv === "string" && cv.trim() !== "") {
            content[field.customVariantKey as string] = cv;
          }
        }
      }
      return content;
    },
    [fields, answers],
  );


  // ── 步骤提交：保存答案 → 下一步或最终提交 ──
  const handleStepSubmit = useCallback(
    (value: unknown, fieldKey?: string) => {
      if (!currentField) return;
      const ek = fieldKey ?? (currentField.key as string);
      const nextAnswers = { ...answers, [ek]: value };
      if (isLastStep) {
        const content = buildContent({ [ek]: value });
        onRespond(content);
      } else {
        setAnswers(nextAnswers);
        setCurrentStep((prev) => prev + 1);
      }
    },
    [answers, currentField, isLastStep, buildContent, onRespond],
  );


  // ── 跳过当前步骤 ──
  const handleSkip = useCallback(() => {
    if (!currentField) return;
    if (isLastStep) {
      const content = buildContent();
      onRespond(content);
    } else {
      setCurrentStep((prev) => prev + 1);
    }
  }, [currentField, isLastStep, buildContent, onRespond]);

  // ── 回退到上一步 ──
  const handleBack = useCallback(() => {
    if (currentStep > 0) {
      setCurrentStep((prev) => prev - 1);
    }
  }, [currentStep]);

  // ── 已确认状态：只读展示 ──
  if (confirmedContent) {
    const choice = formatConfirmedChoice(schema, confirmedContent);
    return (
      <Card className="my-2 border-primary/20 bg-background">
        <CardContent className="flex items-center gap-2 py-3 text-sm">
          <Check className="size-4 text-primary shrink-0" />
          <span className="text-muted-foreground">{message}</span>
          <span className="font-medium">{choice}</span>
        </CardContent>
      </Card>
    );
  }

  if (!currentField) {
    return null;
  }

  const actionLabel = isLastStep
    ? t("acp.elicitation.submit", "提交")
    : t("acp.elicitation.next", "下一步");
  const questionText = stepMessage(
    message,
    currentField.description ?? currentField.title,
    currentStep,
    t("acp.elicitation.questionFallback", "请选择一个答案"),
  );

  // ── 进度指示器 ──
  const ProgressDots = isMultiStep ? (
    <div className="flex items-center justify-center gap-1.5 mb-1">
      {fields.map((_, i) => (
        <span
          key={i}
          className={cn(
            "size-1.5 rounded-full transition-colors",
            i < currentStep
              ? "bg-primary/40"
              : i === currentStep
                ? "bg-primary"
                : "bg-muted-foreground/20",
          )}
        />
      ))}
      <span className="text-[10px] text-muted-foreground ml-1">
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
    <Card className="my-2 border-primary/30">
      <CardContent className="space-y-3 pt-4">
        {/* 进度指示器 */}
        {ProgressDots}

        {/* 当前问题文本 */}
        {questionText && (
          <div className="text-sm text-muted-foreground">
            <Markdown>{questionText}</Markdown>
          </div>
        )}

        {/* 单选：选中态 + 确认按钮 */}
        {currentField.isSelect && !currentField.isMulti && (
          <div className="space-y-1">
            {!customActive ? (<>
              {currentField.options!.map((o) => {
                const sel = selectedValue === o.value;
                return (<button key={o.value} type="button" onClick={() => setSelectedValue(o.value)}
                  className={cn("w-full flex items-center justify-between text-left rounded-md border px-3 py-2.5 text-sm transition-all",
                    sel ? "border-primary bg-primary/5 shadow-sm" : "hover:border-primary/40 hover:bg-muted/50",
                    "active:scale-[0.995]", "disabled:opacity-50 disabled:cursor-not-allowed")}
                ><span className="font-medium">{o.label}</span>
                  {sel ? <Check className="size-4 text-primary shrink-0" /> : <ChevronRight className="size-4 opacity-0 transition-opacity group-hover:opacity-50 text-muted-foreground" />}</button>);
              })}
              {currentField.hasCustomVariant && (
                <button type="button" onClick={() => { setCustomActive(true); setSelectedValue(null); }}
                  className={cn("w-full flex items-center gap-2 rounded-md border border-dashed px-3 py-2.5 text-sm text-muted-foreground transition-colors",
                    "hover:border-primary hover:text-foreground")}
                ><Pencil className="size-4" /><span>{t("acp.elicitation.customPlaceholder", "其他答案...")}</span></button>
              )}
            </>) : (
              <div className="space-y-2">
                <button type="button" onClick={() => { setCustomActive(false); setCustomText(""); }}
                  className={cn("text-xs text-muted-foreground hover:text-foreground transition-colors")}
                >← {t("acp.elicitation.backToOptions", "返回选项")}</button>
                <div className="flex gap-2">
                  <Input autoFocus value={customText}
                    onChange={(e) => setCustomText(e.target.value)}
                    onKeyDown={(e) => { if (e.key === "Enter" && customText.trim()) {
                      handleStepSubmit(customText.trim(), currentField.customVariantKey); }}}
                    placeholder={currentField.customVariantDescription || t("acp.elicitation.customPlaceholder", "输入答案后按回车...")}
                    className="flex-1" />
                </div>
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
                  onClick={() =>
                    setMultiValues((prev) =>
                      selected
                        ? prev.filter((v) => v !== option.value)
                        : [...prev, option.value],
                    )
                  }
                  className={cn(
                    "w-full flex items-center gap-3 text-left rounded-md border px-3 py-2.5 text-sm transition-all",
                    selected
                      ? "border-primary bg-primary/5"
                      : "hover:bg-muted/50",
                    "active:scale-[0.995]",
                    "disabled:opacity-50",
                  )}
                >
                  <span
                    className={cn(
                      "size-4 rounded border-2 flex items-center justify-center shrink-0 transition-colors",
                      selected
                        ? "border-primary bg-primary"
                        : "border-muted-foreground/30",
                    )}
                  >
                    {selected && (
                      <Check className="size-3 text-primary-foreground" />
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
                  "w-full flex items-center gap-2 rounded-md border border-dashed px-3 py-2.5 text-sm text-muted-foreground transition-colors",
                  "hover:border-primary hover:text-foreground",
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
        <div className="flex items-center justify-between pt-1">
          {BackButton}
          <div className="flex items-center gap-2 ml-auto">
            {/* 跳过按钮（非必填字段） */}
            {!currentIsRequired && (
              <button
                type="button"
                onClick={handleSkip}
                className={cn(
                  "text-xs text-muted-foreground hover:text-foreground transition-colors px-2 py-1",
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
                className={cn("inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors",
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
                className={cn("inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors",
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
