/**
 * WebSocket client — singleton connection manager with auto-reconnect,
 * topic-based pub/sub, and SolidJS reactive status signal.
 *
 * Protocol matches the Rust server in crates/server/src/ws.rs:
 *   ClientMessage: subscribe | unsubscribe
 *   ServerMessage: event | error | subscribed | unsubscribed
 */

import { createSignal } from "solid-js";
import type { ClientMessage, ConnectionStatus, ServerMessage } from "../types";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Handler called when a server message arrives for a subscribed topic. */
export type MessageHandler = (msg: ServerMessage) => void;

// ---------------------------------------------------------------------------
// Reconnect configuration
// ---------------------------------------------------------------------------

const RECONNECT_BASE_MS = 2000;
const RECONNECT_MAX_MS = 30_000;
const RECONNECT_JITTER_MS = 200;

function backoff(attempt: number): number {
  const delay = Math.min(
    RECONNECT_BASE_MS * Math.pow(2, attempt),
    RECONNECT_MAX_MS,
  );
  return delay + Math.random() * RECONNECT_JITTER_MS;
}

// ---------------------------------------------------------------------------
// Singleton state
// ---------------------------------------------------------------------------

const [status, setStatus] = createSignal<ConnectionStatus>("disconnected");

let socket: WebSocket | null = null;
let reconnectAttempt = 0;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let manualClose = false;

/** Active subscriptions: topic → set of handlers. */
const subscriptions = new Map<string, Set<MessageHandler>>();

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

function sendRaw(msg: ClientMessage): void {
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify(msg));
  }
}

function resubscribeAll(): void {
  for (const topic of subscriptions.keys()) {
    sendRaw({ type: "subscribe", topic });
  }
}

function dispatch(msg: ServerMessage): void {
  if (msg.type === "event") {
    const handlers = subscriptions.get(msg.topic);
    if (handlers) {
      for (const h of handlers) h(msg);
    }
    // Also dispatch to wildcard handlers keyed on "*"
    const wildcardHandlers = subscriptions.get("*");
    if (wildcardHandlers) {
      for (const h of wildcardHandlers) h(msg);
    }
  }
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

export function connect(url = "/ws"): void {
  if (
    socket?.readyState === WebSocket.OPEN ||
    socket?.readyState === WebSocket.CONNECTING
  ) {
    return;
  }

  manualClose = false;
  // Only show "connecting" on the first attempt — on reconnects, stay
  // "disconnected" until actually connected to avoid status bar flicker.
  if (reconnectAttempt === 0) {
    setStatus("connecting");
  }

  const ws = new WebSocket(url);
  socket = ws;

  ws.addEventListener("open", () => {
    reconnectAttempt = 0;
    setStatus("connected");
    resubscribeAll();
  });

  ws.addEventListener("message", (event) => {
    try {
      const msg = JSON.parse(event.data as string) as ServerMessage;
      dispatch(msg);
    } catch {
      console.warn("[ws] Failed to parse server message:", event.data);
    }
  });

  ws.addEventListener("close", () => {
    socket = null;
    if (manualClose) {
      setStatus("disconnected");
      return;
    }
    setStatus("disconnected");
    scheduleReconnect();
  });

  ws.addEventListener("error", () => {
    // Skip the transient "error" state — the close event fires immediately
    // after and sets "disconnected", which avoids a status flash in the UI.
    setStatus("disconnected");
  });
}

function scheduleReconnect(): void {
  if (reconnectTimer !== null) return;
  const delay = backoff(reconnectAttempt++);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, delay);
}

export function disconnect(): void {
  manualClose = true;
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  socket?.close();
  socket = null;
  setStatus("disconnected");
}

// ---------------------------------------------------------------------------
// Subscribe / unsubscribe
// ---------------------------------------------------------------------------

/**
 * Subscribe to a topic. Returns an unsubscribe function.
 *
 * The topic can contain wildcards as understood by the server (e.g. "task:*").
 * The special topic "*" subscribes to all incoming event messages on the client side.
 */
export function subscribe(topic: string, handler: MessageHandler): () => void {
  if (!subscriptions.has(topic)) {
    subscriptions.set(topic, new Set());
    // Tell the server if we have a live connection
    if (topic !== "*") {
      sendRaw({ type: "subscribe", topic });
    }
  }
  subscriptions.get(topic)!.add(handler);

  return () => unsubscribe(topic, handler);
}

export function unsubscribe(topic: string, handler: MessageHandler): void {
  const handlers = subscriptions.get(topic);
  if (!handlers) return;
  handlers.delete(handler);
  if (handlers.size === 0) {
    subscriptions.delete(topic);
    if (topic !== "*") {
      sendRaw({ type: "unsubscribe", topic });
    }
  }
}

// ---------------------------------------------------------------------------
// Reactive hook
// ---------------------------------------------------------------------------

/**
 * SolidJS hook — returns reactive connection status and subscribe helper.
 *
 * ```ts
 * const { status, subscribe } = useWebSocket();
 * ```
 */
export function useWebSocket() {
  return {
    /** Reactive signal — updates when connection state changes. */
    status,
    /** Subscribe to a server topic. Returns cleanup function. */
    subscribe,
    /** Imperatively disconnect. */
    disconnect,
    /** Imperatively (re)connect. */
    connect,
  };
}

// ---------------------------------------------------------------------------
// Project topic helper
// ---------------------------------------------------------------------------

/**
 * Build a project-namespaced WS topic string.
 * e.g. projectTopic("proj-123", "board:update") → "project:proj-123:board:update"
 */
export function projectTopic(projectId: string, topic: string): string {
  return `project:${projectId}:${topic}`;
}

// ---------------------------------------------------------------------------
// Format helpers (used in tests)
// ---------------------------------------------------------------------------

/** Serialise a ClientMessage to the JSON wire format. */
export function formatClientMessage(msg: ClientMessage): string {
  return JSON.stringify(msg);
}
