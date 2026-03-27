export type SuggestionKind = "complete" | "partial";

export interface Suggestion {
  kind: SuggestionKind;
  /** Display text shown on the button */
  text: string;
  /**
   * Full message sent (complete) or pre-filled in input (partial).
   * Defaults to `text` if omitted.
   */
  value?: string;
}
