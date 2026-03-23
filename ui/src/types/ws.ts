/**
 * WebSocket protocol types matching the Rust server in crates/server/src/ws.rs.
 *
 * ClientMessage — sent from browser to server.
 * ServerMessage — sent from server to browser.
 */

// ---------------------------------------------------------------------------
// Client → Server
// ---------------------------------------------------------------------------

export type ClientMessage =
  | { type: "subscribe"; topic: string }
  | { type: "unsubscribe"; topic: string };

// ---------------------------------------------------------------------------
// Server → Client
// ---------------------------------------------------------------------------

export type ServerMessage =
  | { type: "event"; topic: string; payload: unknown }
  | { type: "error"; message: string }
  | { type: "subscribed"; topic: string }
  | { type: "unsubscribed"; topic: string };

// ---------------------------------------------------------------------------
// Connection state
// ---------------------------------------------------------------------------

export type ConnectionStatus =
  | "connecting"
  | "connected"
  | "disconnected"
  | "error";
