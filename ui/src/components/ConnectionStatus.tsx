import type { Component } from "solid-js";
import type { ConnectionStatus } from "../types";

interface Props {
  status: ConnectionStatus;
}

const statusLabel: Record<ConnectionStatus, string> = {
  connected: "Connected",
  connecting: "Connecting…",
  disconnected: "Disconnected",
  error: "Error",
};

const statusColor: Record<ConnectionStatus, string> = {
  connected: "#22c55e",
  connecting: "#f59e0b",
  disconnected: "#6b7280",
  error: "#ef4444",
};

const ConnectionStatusBadge: Component<Props> = (props) => {
  return (
    <span
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "6px",
        "font-size": "0.75rem",
        color: statusColor[props.status],
      }}
    >
      <span
        style={{
          width: "8px",
          height: "8px",
          "border-radius": "50%",
          background: statusColor[props.status],
          display: "inline-block",
        }}
      />
      {statusLabel[props.status]}
    </span>
  );
};

export default ConnectionStatusBadge;
