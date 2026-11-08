import { create } from "zustand";

export type ToastVariant = "success" | "error" | "info";

export interface ToastItem {
  id: number;
  variant: ToastVariant;
  title: string;
  message?: string;
}

interface ToastStore {
  toasts: ToastItem[];
  push: (toast: Omit<ToastItem, "id">) => void;
  dismiss: (id: number) => void;
}

let sequence = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export const useToastStore = create<ToastStore>((set, get) => ({
  toasts: [],

  push: (item) => {
    const id = ++sequence;
    // Keep at most four stacked toasts; older ones are dropped with their timer.
    set((state) => {
      const next = [...state.toasts, { ...item, id }];
      while (next.length > 4) {
        const removed = next.shift();
        if (removed) {
          const timer = timers.get(removed.id);
          if (timer) clearTimeout(timer);
          timers.delete(removed.id);
        }
      }
      return { toasts: next };
    });

    const ttl = item.variant === "error" ? 8000 : 4500;
    timers.set(
      id,
      setTimeout(() => get().dismiss(id), ttl),
    );
  },

  dismiss: (id) => {
    const timer = timers.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.delete(id);
    }
    set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) }));
  },
}));

/** Imperative helpers so non-React code can raise toasts too. */
export const toast = {
  success: (title: string, message?: string) =>
    useToastStore.getState().push({ variant: "success", title, message }),
  error: (title: string, message?: string) =>
    useToastStore.getState().push({ variant: "error", title, message }),
  info: (title: string, message?: string) =>
    useToastStore.getState().push({ variant: "info", title, message }),
};
