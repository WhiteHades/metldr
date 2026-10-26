/*
 * Layout behavior adapted from IceCreamHarness.tsx in beautiful-ui.
 * Copyright (c) 2026 Shane Levine. Licensed under the MIT License.
 * https://github.com/slev12397/beautiful-ui
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import "./chat-layout.css";

type ChatTab = { id: string; title: string };

type ChatContent =
  | { kind: "empty"; body: ReactNode }
  | { kind: "thread"; body: ReactNode };

type ChatPane = {
  id: string;
  title: string;
  content: ReactNode;
  onClose(): void;
};

type ChatLayoutProps = {
  activeId: string;
  tabs: readonly ChatTab[];
  navigationDisabled: boolean;
  onSelect(id: string): void;
  onClose(id: string): void;
  onNew(): void;
  sidebar?: ReactNode;
  pane?: ChatPane;
  content: ChatContent;
  composer: ReactNode;
  status: ReactNode;
};

export function ChatLayout({
  activeId,
  tabs,
  navigationDisabled,
  onSelect,
  onClose,
  onNew,
  sidebar,
  pane,
  content,
  composer,
  status,
}: ChatLayoutProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLDivElement>(null);
  const [composerHeight, setComposerHeight] = useState(0);

  useLayoutEffect(() => {
    if (content.kind !== "thread") {
      setComposerHeight(0);
      return;
    }
    const scroll = scrollRef.current;
    const composerElement = composerRef.current;
    if (!scroll || !composerElement) return;

    const measureComposer = () =>
      setComposerHeight(composerElement.offsetHeight);
    measureComposer();
    const composerObserver = new ResizeObserver(measureComposer);
    composerObserver.observe(composerElement);
    return () => composerObserver.disconnect();
  }, [activeId, content.kind]);

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    const contentElement = scroll?.firstElementChild;
    if (content.kind !== "thread" || !scroll || !contentElement) return;

    let follow = true;
    let frame = 0;
    const updateFollow = () => {
      follow =
        scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 120;
    };
    const pin = () => {
      if (!follow) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        scroll.scrollTop = scroll.scrollHeight;
      });
    };

    scroll.scrollTop = scroll.scrollHeight;
    scroll.addEventListener("scroll", updateFollow, { passive: true });
    const resizeObserver = new ResizeObserver(pin);
    resizeObserver.observe(contentElement);
    const mutationObserver = new MutationObserver(pin);
    mutationObserver.observe(contentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["style"],
    });

    return () => {
      scroll.removeEventListener("scroll", updateFollow);
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [activeId, content.kind]);

  const composerClearance = composerHeight + (status ? 44 : 16);

  return (
    <main className="chat-layout" data-chat-layout>
      {sidebar && <div className="chat-layout__sidebar">{sidebar}</div>}
      <div className="chat-layout__workspace">
        <section className="chat-layout__main">
          <div
            className="chat-layout__tabs"
            data-chat-tabs
            role="group"
            aria-label="Chats"
          >
            {tabs.map((tab) => (
              <div
                className="chat-layout__tab"
                data-active={tab.id === activeId || undefined}
                key={tab.id}
              >
                <button
                  className="chat-layout__tab-select"
                  data-chat-tab-id={tab.id}
                  type="button"
                  aria-pressed={tab.id === activeId}
                  title={tab.title}
                  disabled={navigationDisabled}
                  onClick={() => onSelect(tab.id)}
                >
                  <span>{tab.title}</span>
                </button>
                <button
                  className="chat-layout__icon-button chat-layout__tab-close"
                  data-chat-close-id={tab.id}
                  type="button"
                  aria-label={`Close ${tab.title}`}
                  title={`Close ${tab.title}`}
                  disabled={navigationDisabled}
                  onClick={() => onClose(tab.id)}
                >
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M18 6 6 18M6 6l12 12" />
                  </svg>
                </button>
              </div>
            ))}
            <button
              className="chat-layout__icon-button chat-layout__new"
              type="button"
              aria-label="New chat"
              title="New chat"
              disabled={navigationDisabled}
              onClick={onNew}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 5v14M5 12h14" />
              </svg>
            </button>
          </div>

          <div className="chat-layout__thread-shell">
            <div
              className="chat-layout__scroll"
              data-chat-scroll
              ref={scrollRef}
              style={content.kind === "thread"
                ? { scrollPaddingBlockEnd: composerClearance }
                : undefined}
            >
              <div
                className={`chat-layout__body chat-layout__body--${content.kind}`}
                style={content.kind === "thread"
                  ? { paddingBlockEnd: composerClearance }
                  : undefined}
              >
                <div className="chat-layout__measure">
                  {content.body}
                  {content.kind === "empty" && composer}
                </div>
              </div>
            </div>
            {content.kind === "thread" && (
              <>
                <div
                  className="chat-layout__fade"
                  aria-hidden="true"
                  style={{ blockSize: composerHeight + (status ? 56 : 32) }}
                />
                <div className="chat-layout__composer" ref={composerRef}>
                  <div className="chat-layout__measure">{composer}</div>
                </div>
                {status && (
                  <div
                    className="chat-layout__status"
                    role="status"
                    aria-live="polite"
                    style={{ insetBlockEnd: composerHeight + 8 }}
                  >
                    {status}
                  </div>
                )}
              </>
            )}
          </div>
        </section>

        {pane && (
          <aside
            className="chat-layout__pane"
            aria-label={pane.title}
            key={pane.id}
          >
            <header className="chat-layout__pane-header">
              <span>{pane.title}</span>
              <button
                className="chat-layout__icon-button"
                type="button"
                aria-label={`Close ${pane.title}`}
                title={`Close ${pane.title}`}
                onClick={pane.onClose}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </header>
            <div className="chat-layout__pane-content">{pane.content}</div>
          </aside>
        )}
      </div>
    </main>
  );
}
