import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Check,
  Circle,
  CircleAlert,
  Copy,
  Inbox,
  LoaderCircle,
  X,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
export function Status({ value }: { value: string }) {
  const pass = [
    "CONSISTENT",
    "PASS",
    "MEASURED",
    "LANDED_OK",
    "SCORED",
    "VERIFIED",
  ].includes(value);
  const fail = [
    "INCONSISTENT",
    "FAIL",
    "LANDED_FAILED",
    "LANDED_THEN_DROPPED",
    "EXPIRED",
    "REJECTED",
    "INVALID",
  ].includes(value);
  const Symbol = pass
    ? Check
    : fail
      ? X
      : ["PENDING", "RUNNING", "LOCKED"].includes(value)
        ? Circle
        : TriangleAlert;
  return (
    <span className={`status ${pass ? "pass" : fail ? "fail" : "warn"}`}>
      <Symbol size={13} aria-hidden="true" />
      {value.replaceAll("_", " ")}
    </span>
  );
}
export function Panel({
  title,
  caption,
  action,
  children,
  className = "",
}: {
  title: string;
  caption?: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel ${className}`}>
      <div className="panel-head">
        <div>
          <h2>{title}</h2>
          {caption && <p>{caption}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
export function Header({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  description: string;
  children?: ReactNode;
}) {
  return (
    <div className="page-head">
      <div>
        <div className="eyebrow">{eyebrow}</div>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      <div className="head-actions">{children}</div>
    </div>
  );
}
export function Empty({
  title,
  text,
  icon: Icon = Inbox,
  children,
}: {
  title: string;
  text: string;
  icon?: LucideIcon;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <span className="empty-symbol">
        <Icon size={25} strokeWidth={1.3} aria-hidden="true" />
      </span>
      <h3>{title}</h3>
      <p>{text}</p>
      {children}
    </div>
  );
}
export function Loading({ text = "Loading evidence…" }: { text?: string }) {
  return (
    <div className="loading" role="status">
      <LoaderCircle size={18} className="spinner" aria-hidden="true" />
      {text}
    </div>
  );
}
export function Notice({
  children,
  danger = false,
}: {
  children: ReactNode;
  danger?: boolean;
}) {
  return (
    <div
      className={`notice ${danger ? "danger" : ""}`}
      role={danger ? "alert" : undefined}
    >
      <CircleAlert size={16} aria-hidden="true" />
      <div>{children}</div>
    </div>
  );
}
export function CopyButton({
  text,
  label = "Copy",
}: {
  text: string;
  label?: string;
}) {
  const [status, setStatus] = useState("");
  useEffect(() => {
    if (!status) return;
    const timer = setTimeout(() => setStatus(""), 2500);
    return () => clearTimeout(timer);
  }, [status]);
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setStatus("Copied");
        } catch {
          setStatus("Copy unavailable");
        }
      }}
    >
      <Copy size={14} aria-hidden="true" />
      {status || label}
      <span className="sr-only" role="status">
        {status}
      </span>
    </button>
  );
}
export function Dialog({
  open,
  close,
  title,
  children,
}: {
  open: boolean;
  close: () => void;
  title: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (open && d && !d.open) d.showModal();
    else if (!open) d?.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-label={title}
      onCancel={close}
      onClick={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="dialog-head">
        <h2>{title}</h2>
        <button
          className="icon-button"
          aria-label="Close dialog"
          onClick={close}
        >
          <X size={18} />
        </button>
      </div>
      {children}
    </dialog>
  );
}
export function EvidenceMeta({
  interval,
  n,
  seconds,
  label = "95% interval",
}: {
  interval: number[];
  n: number;
  seconds?: number | null;
  label?: string;
}) {
  return (
    <div className="evidence-meta">
      <span>
        {label} {(interval[0] * 100).toFixed(1)}–
        {(interval[1] * 100).toFixed(1)}%
      </span>
      <span>
        n<sub>eff</sub> {n.toFixed(1)}
      </span>
      <span>
        {seconds == null ? "Age unavailable" : `${seconds.toFixed(1)}s old`}
      </span>
    </div>
  );
}
