/**
 * KeyboardManager — global keyboard event handler.
 *
 * Mount at app level (inside AppLayout) to activate all keyboard bindings.
 * Manages a chord state machine, command palette, and help overlay.
 */

import type { ParentComponent } from "solid-js";
import { createSignal, onMount, onCleanup } from "solid-js";
import { useNavigate, useLocation } from "@solidjs/router";
import { processChord, createChordState, type ChordState } from "./chords";
import CommandPalette from "./CommandPalette";
import HelpOverlay from "./HelpOverlay";

// ---------------------------------------------------------------------------
// View context helpers
// ---------------------------------------------------------------------------

type ViewContext = "triage" | "board" | "agents" | "mission-control" | "other";

function getViewContext(pathname: string): ViewContext {
  if (pathname === "/" || pathname === "/mission-control") return "mission-control";
  if (pathname.startsWith("/triage")) return "triage";
  if (pathname.startsWith("/board")) return "board";
  if (pathname.startsWith("/agents")) return "agents";
  return "other";
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const KeyboardManager: ParentComponent = (props) => {
  const navigate = useNavigate();
  const location = useLocation();

  const [showPalette, setShowPalette] = createSignal(false);
  const [showHelp, setShowHelp] = createSignal(false);

  // Chord state is mutable plain object — not a signal, intentionally.
  // We don't need reactivity here; only the result of a chord matters.
  let chordState: ChordState = createChordState();

  const isOverlayOpen = () => showPalette() || showHelp();

  const handleKeyDown = (e: KeyboardEvent) => {
    // Ignore events from inputs/textareas to avoid interfering with typing
    const target = e.target as HTMLElement;
    const tag = target.tagName.toLowerCase();
    if (tag === "input" || tag === "textarea" || target.isContentEditable) {
      // Allow Escape to close overlays even from inputs
      if (e.key === "Escape" && isOverlayOpen()) {
        setShowPalette(false);
        setShowHelp(false);
      }
      return;
    }

    // Cmd+K / Ctrl+K — command palette
    if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      setShowPalette((v) => !v);
      setShowHelp(false);
      return;
    }

    // Don't process other bindings while an overlay is open
    if (isOverlayOpen()) return;

    const key = e.key;

    // Help overlay
    if (key === "?") {
      e.preventDefault();
      setShowHelp(true);
      return;
    }

    // Escape — collapse / go back (handled per-view; here we just clear chord)
    if (key === "Escape") {
      chordState = createChordState();
      return;
    }

    // Process chord machine
    const { next, result } = processChord(chordState, key);
    chordState = next;

    if (result) {
      e.preventDefault();
      switch (result.action) {
        case "goto-triage":
          navigate("/triage");
          break;
        case "goto-board":
          navigate("/board");
          break;
        case "goto-agents":
          navigate("/agents");
          break;
        case "goto-mission-control":
          navigate("/");
          break;
      }
      return;
    }

    // If chord is pending (waiting for second key), consume this event
    if (chordState.pending !== null) {
      e.preventDefault();
      return;
    }

    // Single-key bindings (context-aware)
    const ctx = getViewContext(location.pathname);

    switch (key) {
      case "j":
        // Emit a custom event that list views can listen to
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("molt:nav-down"));
        break;
      case "k":
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("molt:nav-up"));
        break;
      case "Enter":
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("molt:nav-enter"));
        break;
      case "h":
        if (ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:nav-left"));
        }
        break;
      case "l":
        if (ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:nav-right"));
        }
        break;
      case "f":
        if (ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:filter-toggle"));
        }
        break;
      case "[":
        if (ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:sidebar-toggle"));
        }
        break;
      case "Tab":
        if (ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:focus-zone-switch"));
        }
        break;
      case "a":
        if (ctx === "triage" || ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:triage-approve"));
        }
        break;
      case "r":
        if (ctx === "triage" || ctx === "mission-control") {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent("molt:triage-reject"));
        }
        break;
      default:
        break;
    }
  };

  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener("keydown", handleKeyDown);
  });

  return (
    <>
      {props.children}

      <CommandPalette
        open={showPalette()}
        onOpenChange={setShowPalette}
        onShowHelp={() => {
          setShowPalette(false);
          setShowHelp(true);
        }}
      />

      <HelpOverlay open={showHelp()} onOpenChange={setShowHelp} />
    </>
  );
};

export default KeyboardManager;
