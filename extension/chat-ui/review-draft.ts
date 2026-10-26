import { useEffect, useState } from "react";

type SavedDraft = { kind: "empty" } | { kind: "saved"; text: string } | { kind: "unavailable" };

function readDraft(key: string): SavedDraft {
  try {
    const text = sessionStorage.getItem(key);
    return text === null ? { kind: "empty" } : { kind: "saved", text };
  } catch {
    return { kind: "unavailable" };
  }
}

export function useReviewDraft(threadId: string) {
  const key = `metldr:review-draft:${threadId}`;
  const [saved] = useState(() => readDraft(key));
  const [text, setText] = useState(saved.kind === "saved" ? saved.text : "");
  const [error, setError] = useState(saved.kind === "unavailable" ? "Your saved draft could not be opened." : "");

  useEffect(() => {
    if (saved.kind !== "saved") return;
    try { sessionStorage.removeItem(key); }
    catch { setError("Your draft is shown, but its saved copy could not be cleared."); }
  }, [key, saved]);

  function review() {
    setError("");
    try { sessionStorage.setItem(key, text); }
    catch {
      setError("Your draft could not be kept. Copy it before reloading.");
      return;
    }
    window.location.reload();
  }

  return { text, setText, error, review };
}
