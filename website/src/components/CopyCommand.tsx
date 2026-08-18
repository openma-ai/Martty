import { useRef, useState } from "react";

interface CopyCommandProps {
  /** The lines shown in the terminal block, in order. */
  lines: readonly string[];
  /** Accessible label for the block, e.g. "Demo install command". */
  label: string;
}

/**
 * A terminal-styled command block with a real copy-to-clipboard button. This
 * is the one genuinely interactive control in the hero, so it is the one
 * piece of markup that needs hydration.
 */
export function CopyCommand({ lines, label }: CopyCommandProps) {
  const [copied, setCopied] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const text = lines.join("\n");

  async function handleCopy() {
    try {
      if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        return;
      }
      setCopied(true);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      timeoutRef.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard permission denied: leave the command selectable in place.
    }
  }

  return (
    <div className="term-block" role="group" aria-label={label}>
      <pre className="term-block__code">
        <code>
          {lines.map((line, i) => (
            <span className="term-block__line" data-prompt={i === 0 ? "$" : "\u203a"} key={line}>
              {line}
            </span>
          ))}
        </code>
      </pre>
      <button type="button" className="term-block__copy" onClick={handleCopy}>
        {copied ? "Copied" : "Copy"}
      </button>
      <span className="sr-only" role="status" aria-live="polite">
        {copied ? "Command copied to clipboard" : ""}
      </span>
    </div>
  );
}
