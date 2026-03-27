/**
 * AiTutorView — wrapper page for the AI Tutor chat.
 *
 * Hardcodes sessionId="default" as a placeholder until the backend
 * session management is implemented.
 */

import type { Component } from "solid-js";
import AiTutorChat from "./AiTutorChat";

const AiTutorView: Component = () => {
  return <AiTutorChat sessionId="default" />;
};

export default AiTutorView;
