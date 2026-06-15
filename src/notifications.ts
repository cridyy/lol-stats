import type { InjectionKey } from "vue"

export type ToastKind = "info" | "success" | "warning" | "error"

export type AppToast = {
  id: number
  kind: ToastKind
  title: string
  message?: string
  actionLabel?: string
  duration?: number
  onAction?: () => void
}

export type ToastPayload = Omit<AppToast, "id">

export type Notify = (toast: ToastPayload) => number

export const notifyKey: InjectionKey<Notify> = Symbol("notify")
