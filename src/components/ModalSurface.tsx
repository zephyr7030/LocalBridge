import { useEffect, useRef, type PropsWithChildren } from "react";

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

// Nested modal layers can overlap in lifetime, so inert ownership must be reference-counted.
const inertOwners = new WeakMap<HTMLElement, { count: number; original: boolean }>();
const modalStackListeners = new Set<() => void>();

function acquireInert(element: HTMLElement) {
  const existing = inertOwners.get(element);
  if (existing) existing.count += 1;
  else inertOwners.set(element, { count: 1, original: element.inert });
  element.inert = true;
}

function releaseInert(element: HTMLElement) {
  const existing = inertOwners.get(element);
  if (!existing) return;
  existing.count -= 1;
  if (existing.count > 0) return;
  element.inert = existing.original;
  inertOwners.delete(element);
}

function modalPriority(element: HTMLElement): number {
  return Number(element.dataset.modalPriority ?? 0);
}

// Priority mirrors the visual layers; later DOM order wins only when priorities tie.
function modalOutranks(candidate: HTMLElement, current: HTMLElement): boolean {
  const candidatePriority = modalPriority(candidate);
  const currentPriority = modalPriority(current);
  if (candidatePriority !== currentPriority) return candidatePriority > currentPriority;
  return Boolean(current.compareDocumentPosition(candidate) & Node.DOCUMENT_POSITION_FOLLOWING);
}

function topModal(): HTMLElement | null {
  let top: HTMLElement | null = null;
  for (const candidate of document.querySelectorAll<HTMLElement>("[data-modal-priority]")) {
    if (!top || modalOutranks(candidate, top)) top = candidate;
  }
  return top;
}

function notifyModalStackChanged() {
  for (const listener of modalStackListeners) listener();
}

export function ModalSurface({
  variant = "dialog",
  className,
  backdropClassName,
  priority = 10,
  initialFocus = "first",
  ariaLabel,
  labelledBy,
  onDismiss,
  dismissOnBackdrop = false,
  children,
}: PropsWithChildren<{
  variant?: "dialog" | "sheet";
  className?: string;
  backdropClassName?: string;
  priority?: number;
  initialFocus?: "first" | "surface";
  ariaLabel?: string;
  labelledBy?: string;
  onDismiss?: () => void | Promise<void>;
  dismissOnBackdrop?: boolean;
}>) {
  const backdropRef = useRef<HTMLDivElement>(null);
  const surfaceRef = useRef<HTMLElement>(null);
  const dismissRef = useRef(onDismiss);
  dismissRef.current = onDismiss;

  useEffect(() => {
    const backdrop = backdropRef.current;
    const surface = surfaceRef.current;
    if (!backdrop || !surface) return;

    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const inerted: HTMLElement[] = [];

    for (let node: HTMLElement | null = backdrop; node && node !== document.body; node = node.parentElement) {
      const parentElement: HTMLElement | null = node.parentElement;
      if (!parentElement) break;
      for (const siblingElement of Array.from(parentElement.children)) {
        if (siblingElement === node || !(siblingElement instanceof HTMLElement)) continue;
        if (siblingElement.matches("[data-modal-priority]") && modalOutranks(siblingElement, backdrop)) continue;
        acquireInert(siblingElement);
        inerted.push(siblingElement);
      }
    }

    const focusable = () => Array.from(surface.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
    // A lower-priority modal can mount after a higher one, so it must also inert itself dynamically.
    let peerInertOwned = false;
    const syncTopState = () => {
      const isTop = topModal() === backdrop;
      if (!isTop && !peerInertOwned) {
        acquireInert(backdrop);
        peerInertOwned = true;
      } else if (isTop && peerInertOwned) {
        releaseInert(backdrop);
        peerInertOwned = false;
      }
      if (isTop && !surface.contains(document.activeElement)) {
        (initialFocus === "surface" ? surface : (focusable()[0] ?? surface)).focus();
      }
    };

    modalStackListeners.add(syncTopState);
    queueMicrotask(() => { if (surface.isConnected) notifyModalStackChanged(); });

    const onKeyDown = (event: KeyboardEvent) => {
      if (topModal() !== backdrop) return;
      if (event.key === "Escape" && dismissRef.current) {
        event.preventDefault();
        void dismissRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) {
        event.preventDefault();
        surface.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || active === surface || !surface.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || active === surface || !surface.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      modalStackListeners.delete(syncTopState);
      if (peerInertOwned) releaseInert(backdrop);
      for (const element of inerted.reverse()) releaseInert(element);
      const nextTop = topModal();
      if (previousFocus?.isConnected && (!nextTop || nextTop.contains(previousFocus))) previousFocus.focus();
      notifyModalStackChanged();
    };
  }, []);

  const backdropBase = variant === "sheet" ? "sheet-backdrop" : "dialog-backdrop";
  const surfaceBase = variant === "sheet" ? "sheet" : "dialog";

  return (
    <div
      ref={backdropRef}
      data-modal-priority={priority}
      className={[backdropBase, backdropClassName].filter(Boolean).join(" ")}
      onMouseDown={dismissOnBackdrop && onDismiss ? (event) => {
        if (event.target === event.currentTarget) void dismissRef.current?.();
      } : undefined}
    >
      <section
        ref={surfaceRef}
        className={[surfaceBase, className].filter(Boolean).join(" ")}
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        aria-labelledby={labelledBy}
        tabIndex={-1}
      >
        {children}
      </section>
    </div>
  );
}
