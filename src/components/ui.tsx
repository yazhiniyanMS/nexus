import clsx from "clsx";
import { AlertTriangle, Loader2 } from "lucide-react";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "default" | "ghost" | "danger";
  size?: "sm" | "md";
  busy?: boolean;
};

export function Button({ variant = "default", size = "md", busy, className, children, disabled, ...rest }: ButtonProps) {
  return (
    <button
      {...rest}
      disabled={disabled || busy}
      className={clsx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:opacity-45",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
        variant === "primary" && "bg-accent text-black hover:bg-emerald-300",
        variant === "default" && "border border-line bg-raised text-ink hover:bg-[#1c1c20]",
        variant === "ghost" && "text-dim hover:bg-raised hover:text-ink",
        variant === "danger" && "border border-red-500/30 bg-red-500/10 text-red-300 hover:bg-red-500/20",
        className,
      )}
    >
      {busy && <Loader2 size={13} className="animate-spin" />}
      {children}
    </button>
  );
}

const fieldClass =
  "h-8 w-full rounded-md border border-line bg-bg px-2.5 text-[13px] text-ink placeholder:text-faint focus:border-accent/60 focus:outline-none";

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
  green: "border-emerald-500/25 bg-emerald-500/10 text-emerald-300",
  red: "border-red-500/25 bg-red-500/10 text-red-300",
  amber: "border-amber-500/25 bg-amber-500/10 text-amber-300",
  blue: "border-sky-500/25 bg-sky-500/10 text-sky-300",
};

export function Badge({ tone = "neutral", children }: { tone?: keyof typeof tones; children: ReactNode }) {
  return (
    <span className={clsx("inline-flex items-center rounded-full border px-2 py-px text-[11px] font-medium", tones[tone])}>
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
    <div className="selectable flex items-start gap-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[13px] text-red-200">
      <AlertTriangle size={15} className="mt-0.5 shrink-0" />
      <span className="break-words">{message}</span>
    </div>
  );
}

export function Empty({ icon, title, children }: { icon?: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full min-h-48 flex-col items-center justify-center gap-2 p-8 text-center">
      {icon && <div className="text-faint">{icon}</div>}
      <div className="text-[15px] font-semibold">{title}</div>
      {children && <div className="max-w-md text-[13px] leading-relaxed text-dim">{children}</div>}
    </div>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line px-5">
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-[15px] font-semibold">{title}</h1>
        {subtitle && <div className="truncate text-xs text-dim">{subtitle}</div>}
      </div>
      {actions}
    </header>
  );
}

export function Tabs<T extends string>({ tabs, value, onChange }: { tabs: { id: T; label: string; count?: number }[]; value: T; onChange: (t: T) => void }) {
  return (
    <div className="flex shrink-0 gap-1 border-b border-line px-4">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onChange(t.id)}
          className={clsx(
            "-mb-px border-b-2 px-2.5 py-2 text-[13px] font-medium",
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
            line.startsWith("+") && !line.startsWith("+++") && "bg-emerald-500/10 text-emerald-200",
            line.startsWith("-") && !line.startsWith("---") && "bg-red-500/10 text-red-200",
            line.startsWith("@@") && "text-sky-300",
            (line.startsWith("diff ") || line.startsWith("commit ")) && "mt-2 font-semibold text-ink",
          )}
        >
          {line || " "}
        </div>
      ))}
    </pre>
  );
}
