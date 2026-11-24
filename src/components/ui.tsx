import clsx from "clsx";
import { AlertTriangle, Ban, Check, Loader2, X } from "lucide-react";
import { forwardRef, useEffect, useRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, type SelectHTMLAttributes } from "react";

// The building blocks every screen uses. Rules: docs/DESIGN.md (Part 2).

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** primary: the one main action of a view. default/secondary: other actions. ghost: toolbar and row actions. */
  variant?: "primary" | "default" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
  busy?: boolean;
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button({ variant = "default", size = "md", busy, className, children, disabled, ...rest }, ref) {
  return (
    <button
      ref={ref}
      {...rest}
      disabled={disabled || busy}
      className={clsx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:opacity-45",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-sm",
        variant === "primary" && "bg-accent font-semibold text-black hover:bg-accent/85",
        (variant === "default" || variant === "secondary") && "border border-line-strong bg-raised text-ink hover:bg-hover",
        variant === "ghost" && "text-dim hover:bg-raised hover:text-ink",
        variant === "danger" && "border border-danger/40 text-danger hover:bg-danger/10",
        className,
      )}
    >
      {busy && <Loader2 size={13} className="animate-spin" />}
      {children}
    </button>
  );
});

const fieldClass =
  "h-8 w-full rounded-md border border-line bg-bg px-2.5 text-sm text-ink placeholder:text-faint focus:border-lav/60 focus:outline-none";

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={clsx(fieldClass, props.className)} />;
}

export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={clsx(fieldClass, "pr-6", props.className)} />;
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block">
      <div className="mb-1 text-xs font-medium text-dim">{label}</div>
      {children}
      {hint && <div className="mt-1 text-xs text-faint">{hint}</div>}
    </label>
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={clsx("rounded-lg border border-line bg-panel", className)}>{children}</div>;
}

const tones = {
  neutral: "border-line bg-raised text-dim",
  green: "border-accent/25 bg-accent/10 text-accent",
  red: "border-danger/25 bg-danger/10 text-danger",
  amber: "border-warn/25 bg-warn/10 text-warn",
  blue: "border-lav/25 bg-lav/10 text-lav",
};

export function Badge({ tone = "neutral", children }: { tone?: keyof typeof tones; children: ReactNode }) {
  return (
    <span className={clsx("inline-flex items-center rounded-full border px-2 py-px text-2xs font-medium", tones[tone])}>
      {children}
    </span>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 p-6 text-dim">
      <Loader2 size={15} className="animate-spin" /> {label ?? "Loading…"}
    </div>
  );
}

export function ErrorBanner({ message }: { message?: string | null }) {
  if (!message) return null;
  return (
    <div className="selectable flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
      <AlertTriangle size={15} className="mt-0.5 shrink-0" />
      <span className="break-words">{message}</span>
    </div>
  );
}

export function Empty({ icon, title, children }: { icon?: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full min-h-48 flex-col items-center justify-center gap-2 p-8 text-center">
      {icon && <div className="text-faint">{icon}</div>}
      <div className="text-md font-semibold">{title}</div>
      {children && <div className="max-w-md text-sm leading-relaxed text-dim">{children}</div>}
    </div>
  );
}


/** A tab strip. Only the selected tab is in the Tab order; ←/→, Home and End move between tabs. */
export function Tabs<T extends string>({ tabs, value, onChange }: { tabs: { id: T; label: string; count?: number }[]; value: T; onChange: (t: T) => void }) {
  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const i = tabs.findIndex((t) => t.id === value);
    const next =
      e.key === "ArrowRight" ? (i + 1) % tabs.length
      : e.key === "ArrowLeft" ? (i - 1 + tabs.length) % tabs.length
      : e.key === "Home" ? 0
      : e.key === "End" ? tabs.length - 1
      : -1;
    if (next < 0) return;
    e.preventDefault();
    onChange(tabs[next].id);
    const buttons = e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=tab]");
    buttons[next]?.focus();
  };
  return (
    <div role="tablist" onKeyDown={onKeyDown} className="flex shrink-0 gap-1 border-b border-line px-4">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={value === t.id}
          tabIndex={value === t.id ? 0 : -1}
          onClick={() => onChange(t.id)}
          className={clsx(
            "-mb-px border-b-2 px-2.5 py-2 text-sm font-medium",
            value === t.id ? "border-accent text-ink" : "border-transparent text-dim hover:text-ink",
          )}
        >
          {t.label}
          {t.count !== undefined && <span className="ml-1.5 text-xs text-faint">{t.count}</span>}
        </button>
      ))}
    </div>
  );
}

/** Unified diff / patch text with added and removed lines coloured. */
export function DiffView({ text }: { text: string }) {
  return (
    <pre className="min-w-full p-3 font-mono text-xs leading-5">
      {text.split("\n").map((line, i) => (
        <div
          key={i}
          className={clsx(
            "px-2 whitespace-pre",
            line.startsWith("+") && !line.startsWith("+++") && "bg-accent/10 text-accent",
            line.startsWith("-") && !line.startsWith("---") && "bg-danger/10 text-danger",
            line.startsWith("@@") && "text-lav",
            (line.startsWith("diff ") || line.startsWith("commit ")) && "mt-2 font-semibold text-ink",
          )}
        >
          {line || " "}
        </div>
      ))}
    </pre>
  );
}

/** An icon-only action. `label` is both its accessible name and its tooltip. */
export const IconButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; tone?: "default" | "danger" }>(
  function IconButton({ label, active, tone = "default", className, children, ...rest }, ref) {
    return (
      <button
        ref={ref}
        type="button"
        aria-label={label}
        title={rest.title ?? label}
        aria-pressed={active}
        {...rest}
        className={clsx(
          "grid size-7 shrink-0 place-items-center rounded-md disabled:opacity-40",
          active ? "bg-raised text-ink" : "text-faint hover:bg-raised hover:text-ink",
          tone === "danger" && "hover:text-danger",
          className,
        )}
      >
        {children}
      </button>
    );
  },
);

/** A key or key combination, e.g. <Kbd>⌘K</Kbd>. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={clsx("inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-sm border border-line px-1 font-sans text-2xs text-faint", className)}>{children}</kbd>;
}

export type StatusKind = "idle" | "queued" | "running" | "waiting" | "done" | "failed" | "cancelled";

const STATUS: Record<StatusKind, { label: string; cls: string }> = {
  idle: { label: "Not started", cls: "text-faint" },
  queued: { label: "Queued", cls: "text-dim" },
  running: { label: "Running", cls: "text-info" },
  waiting: { label: "Needs you", cls: "text-warn" },
  done: { label: "Done", cls: "text-ok" },
  failed: { label: "Failed", cls: "text-danger" },
  cancelled: { label: "Cancelled", cls: "text-faint" },
};

/** The one way NEXUS shows the state of something long-running. `label` overrides the default wording. */
export function Status({ kind, label, showLabel = true, className }: { kind: StatusKind; label?: string; showLabel?: boolean; className?: string }) {
  const s = STATUS[kind];
  const mark =
    kind === "done" ? <Check size={13} strokeWidth={2.25} />
      : kind === "failed" ? <X size={13} strokeWidth={2.25} />
        : kind === "cancelled" ? <Ban size={12} />
          : (
            <span
              className={clsx(
                "block size-2 rounded-full",
                kind === "idle" || kind === "queued" ? "border border-current" : "bg-current",
                kind === "running" && "animate-status",
              )}
            />
          );
  const text = label ?? s.label;
  return (
    <span data-status={kind} className={clsx("inline-flex shrink-0 items-center gap-1.5 text-xs font-medium", s.cls, className)} title={showLabel ? undefined : text}>
      <span className="grid size-3.5 place-items-center">{mark}</span>
      {showLabel ? text : <span className="sr-only">{text}</span>}
    </span>
  );
}

/**
 * A destructive action that needs a second click on a button naming the consequence.
 * It reverts after six seconds, or with Escape.
 */
export function ConfirmButton({ label, confirm, onConfirm, busy, disabled, title, icon, size = "sm", variant = "ghost" }: {
  label: ReactNode; confirm: string; onConfirm: () => void | Promise<void>; busy?: boolean; disabled?: boolean; title?: string; icon?: ReactNode;
  size?: "sm" | "md"; variant?: "ghost" | "default" | "danger";
}) {
  const [asking, setAsking] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const confirmRef = useRef<HTMLButtonElement>(null);
  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!asking) return;
    confirmRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setAsking(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [asking]);
  if (!asking) {
    return (
      <Button
        size={size}
        variant={variant}
        type="button"
        title={title}
        aria-label={label === "" || label === undefined ? title : undefined}
        disabled={disabled}
        busy={busy}
        onClick={() => {
          setAsking(true);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => setAsking(false), 6000);
        }}
      >
        {icon}
        {label}
      </Button>
    );
  }
  return (
    <span className="inline-flex items-center gap-1" role="group" aria-label="Confirm">
      <Button ref={confirmRef} size={size} variant="danger" type="button" busy={busy} onClick={() => { setAsking(false); void onConfirm(); }}>{confirm}</Button>
      <Button size={size} variant="ghost" type="button" onClick={() => setAsking(false)}>Keep</Button>
    </span>
  );
}

/**
 * The top bar of a view: what it is about, its context, and its actions. 44 px, no subtitle
 * restating the title.
 */
export function ViewHeader({ title, context, actions, children }: { title: ReactNode; context?: ReactNode; actions?: ReactNode; children?: ReactNode }) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
      <h1 className="shrink-0 text-sm font-semibold">{title}</h1>
      {context && <div className="flex min-w-0 items-center gap-2 truncate text-xs text-dim">{context}</div>}
      {children}
      <div className="ml-auto flex shrink-0 items-center gap-2">{actions}</div>
    </header>
  );
}

/** A labelled group inside a pane: small caps-free title, optional trailing actions. */
export function SectionTitle({ children, actions, className }: { children: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={clsx("flex h-7 items-center gap-2", className)}>
      <h2 className="text-xs font-semibold text-dim">{children}</h2>
      <div className="ml-auto flex items-center gap-1">{actions}</div>
    </div>
  );
}
