import { describe, it, expect } from "vitest";
import { formatClientMessage } from "../ws";

describe("formatClientMessage", () => {
  it("serialises a subscribe message to the correct JSON wire format", () => {
    const msg = formatClientMessage({ type: "subscribe", topic: "task:*" });
    expect(JSON.parse(msg)).toEqual({ type: "subscribe", topic: "task:*" });
  });

  it("serialises an unsubscribe message correctly", () => {
    const msg = formatClientMessage({
      type: "unsubscribe",
      topic: "task:01ABCDEF",
    });
    expect(JSON.parse(msg)).toEqual({
      type: "unsubscribe",
      topic: "task:01ABCDEF",
    });
  });

  it("produces a string (not an object)", () => {
    const msg = formatClientMessage({ type: "subscribe", topic: "agent:*" });
    expect(typeof msg).toBe("string");
  });

  it("subscribe message has only type and topic fields", () => {
    const msg = formatClientMessage({ type: "subscribe", topic: "pipeline:x" });
    const parsed = JSON.parse(msg);
    expect(Object.keys(parsed).sort()).toEqual(["topic", "type"]);
  });
});
