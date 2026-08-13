import { cn } from "@/lib/utils";

const ACP_PROCESSING_SPINNER_CLASS_NAME =
  "shrink-0 rounded-full border-2 border-gold-running/30 border-t-gold-running motion-safe:animate-spin motion-reduce:animate-none [animation-duration:900ms] [will-change:transform]";

export function AcpProcessingSpinner({
  className,
}: {
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      data-acp-processing-spinner="true"
      className={cn(ACP_PROCESSING_SPINNER_CLASS_NAME, className)}
    />
  );
}
