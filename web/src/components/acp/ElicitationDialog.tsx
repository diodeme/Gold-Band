import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Markdown } from "@/components/prompt-kit/markdown";
import { cn } from "@/lib/utils";

export interface ElicitationSchema {
  type: "object";
  properties?: Record<string, ElicitationPropertySchema>;
}

export interface ElicitationPropertySchema {
  type: "string" | "array";
  title?: string;
  description?: string;
  oneOf?: Array<{ const: string; title: string }>;
  anyOf?: Array<{ const: string; title: string }>;
}

export interface ElicitationDialogProps {
  open: boolean;
  elicitationId: string;
  message: string;
  schema: ElicitationSchema;
  onRespond: (
    action: "accept" | "decline",
    content?: Record<string, unknown>,
  ) => void;
}

export function ElicitationDialog({
  open,
  elicitationId: _elicitationId,
  message,
  schema,
  onRespond,
}: ElicitationDialogProps) {
  const { t } = useTranslation();
  const [values, setValues] = useState<Record<string, unknown>>({});

  const fields = useMemo(() => {
    if (!schema.properties) return [];
    return Object.entries(schema.properties).map(([key, prop]) => ({
      key,
      ...prop,
    }));
  }, [schema]);

  const allRequiredFilled = useMemo(() => {
    return fields.every((field) => {
      // Validate single-select (oneOf) as required
      if (field.oneOf) {
        const val = values[field.key];
        return val !== undefined && val !== null && val !== "";
      }
      return true;
    });
  }, [fields, values]);

  const handleAccept = () => {
    const content: Record<string, unknown> = {};
    for (const field of fields) {
      if (values[field.key] !== undefined && values[field.key] !== "") {
        content[field.key] = values[field.key];
      }
    }
    onRespond("accept", content);
  };

  const handleDecline = () => {
    onRespond("decline");
  };

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) handleDecline(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("elicitation.title", "需要您的选择")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 py-2">
          {message && (
            <div className="text-sm text-muted-foreground">
              <Markdown>{message}</Markdown>
            </div>
          )}
          {fields.map((field) => (
            <div key={field.key} className="space-y-2">
              {field.title && <Label>{field.title}</Label>}
              {field.description && (
                <p className="text-xs text-muted-foreground">
                  {field.description}
                </p>
              )}
              {/* 单选：oneOf → 原生 radio button 组 */}
              {field.oneOf && field.oneOf.length > 0 && (
                <div className="space-y-1.5">
                  {field.oneOf.map((option) => (
                    <label
                      key={option.const}
                      className={cn(
                        "flex items-center gap-2 rounded-md border px-3 py-2 cursor-pointer transition-colors",
                        values[field.key] === option.const
                          ? "border-primary bg-primary/5"
                          : "hover:bg-muted/50",
                      )}
                    >
                      <input
                        type="radio"
                        name={field.key}
                        value={option.const}
                        checked={values[field.key] === option.const}
                        onChange={() =>
                          setValues((prev) => ({
                            ...prev,
                            [field.key]: option.const,
                          }))
                        }
                        className="size-4 accent-primary"
                      />
                      <span className="text-sm">{option.title}</span>
                    </label>
                  ))}
                </div>
              )}
              {/* 多选：anyOf → 原生 checkbox 组 */}
              {field.anyOf && field.anyOf.length > 0 && (
                <div className="space-y-1.5">
                  {field.anyOf.map((option) => {
                    const selected = Array.isArray(values[field.key])
                      ? (values[field.key] as string[]).includes(option.const)
                      : false;
                    return (
                      <label
                        key={option.const}
                        className={cn(
                          "flex items-center gap-2 rounded-md border px-3 py-2 cursor-pointer transition-colors",
                          selected
                            ? "border-primary bg-primary/5"
                            : "hover:bg-muted/50",
                        )}
                      >
                        <input
                          type="checkbox"
                          checked={selected}
                          onChange={(e) => {
                            setValues((prev) => {
                              const prevArr = Array.isArray(prev[field.key])
                                ? (prev[field.key] as string[])
                                : [];
                              return {
                                ...prev,
                                [field.key]: e.target.checked
                                  ? [...prevArr, option.const]
                                  : prevArr.filter((v) => v !== option.const),
                              };
                            });
                          }}
                          className="size-4 accent-primary"
                        />
                        <span className="text-sm">{option.title}</span>
                      </label>
                    );
                  })}
                </div>
              )}
              {/* 自由文本：无 oneOf/anyOf → Input */}
              {!field.oneOf && !field.anyOf && (
                <Input
                  value={String(values[field.key] ?? "")}
                  onChange={(e) =>
                    setValues((prev) => ({
                      ...prev,
                      [field.key]: e.target.value,
                    }))
                  }
                  placeholder={field.title ?? field.key}
                />
              )}
            </div>
          ))}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={handleDecline}>
            {t("elicitation.skip", "跳过")}
          </Button>
          <Button onClick={handleAccept} disabled={!allRequiredFilled}>
            {t("elicitation.confirm", "确认")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
