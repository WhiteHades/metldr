// Browser text services and semantic DOM accessibility mirror.
const coarse = () => matchMedia('(pointer: coarse)').matches;
const high = n => n >= 0xd800 && n <= 0xdbff;
const low = n => n >= 0xdc00 && n <= 0xdfff;
function boundary(text, at, forward) {
    if (at > 0 && at < text.length && high(text.charCodeAt(at - 1)) && low(text.charCodeAt(at))) at += forward ? 1 : -1;
    return at;
}

function replacement(old, next, composition, caret) {
    let start = 0;
    while (start < old.length && start < next.length && old[start] === next[start]) start++;
    start = boundary(old, start, false);
    let end = old.length, nextEnd = next.length;
    while (end > start && nextEnd > start && old[end - 1] === next[nextEnd - 1]) { end--; nextEnd--; }
    const adjusted = boundary(old, end, true) - end;
    end += adjusted; nextEnd += adjusted;
    if (composition && caret >= composition.length && next.slice(caret - composition.length, caret) === composition) {
        const from = caret - composition.length;
        const to = caret + old.length - next.length;
        if (from <= to && to <= old.length) {
            if (old === next) { start = from; end = to; nextEnd = caret; }
            else { start = Math.min(start, from); end = Math.max(end, to); nextEnd = end + next.length - old.length; }
        }
    }
    return { start, end, text: next.slice(start, nextEnd) };
}

function browserShortcut(event) {
    const chord = event.metaKey || event.ctrlKey;
    if (event.key === 'F5' || event.key === 'F12') return true;
    if (!chord) return false;
    const k = event.key.toLowerCase();
    // Leave copy ('c') un-prevented too so the browser fires its synchronous `copy` event.
    return ['c', 'r', 'l', 't', 'w', 'n', 'f', 'g', '=', '+', '-', '0'].includes(k)
        || (event.altKey && ['i', 'j', 'u'].includes(k));
}

export function mount(canvas, callback, embedded, fitHeight) {
    const textarea = document.createElement('textarea');
    textarea.setAttribute('aria-label', 'Text input');
    textarea.setAttribute('autocapitalize', 'sentences');
    const password = document.createElement('input');
    password.type = 'password';
    password.setAttribute('aria-label', 'Password input');
    password.autocomplete = 'off';
    password.setAttribute('autocorrect', 'off');
    password.setAttribute('autocapitalize', 'off');
    password.spellcheck = false;
    for (const field of [textarea, password]) {
        field.style.cssText = 'position:fixed;left:0;top:0;width:1px;height:1px;padding:0;border:0;opacity:0;font-size:16px;resize:none;pointer-events:none;';
        field.tabIndex = -1;
        field.inputMode = coarse() ? 'none' : 'text';
        field.inert = true;
        field.setAttribute('aria-hidden', 'true');
        document.body.append(field);
    }
    const a11y = document.createElement('div');
    a11y.setAttribute('data-rune-a11y', '');
    a11y.setAttribute('role', 'document');
    if (embedded && canvas.parentElement) {
        if (getComputedStyle(canvas.parentElement).position === 'static') canvas.parentElement.style.position = 'relative';
        a11y.style.cssText = 'position:absolute;inset:0;overflow:hidden;pointer-events:none;z-index:1;color:transparent;';
        canvas.after(a11y);
    } else {
        a11y.style.cssText = 'position:fixed;inset:0;overflow:hidden;pointer-events:none;z-index:1;color:transparent;';
        document.body.append(a11y);
    }
    const h = { canvas, textarea, password, a11y, a11yNodes: new Map(), field: textarea, key: '', base: 0, value: '',
        copyText: '', composing: false, composition: '', shift: false, listeners: [], syncing: false, disposed: false,
        restoreFocus: false, embedded, fitHeight };
    const send = event => {
        const message = JSON.stringify(event);
        if (h.syncing) queueMicrotask(() => { if (!h.disposed) callback(message); });
        else if (!h.disposed) callback(message);
    };
    h.send = send;
    const listen = (target, name, handler, options) => {
        target.addEventListener(name, handler, options);
        h.listeners.push(() => target.removeEventListener(name, handler, options));
    };
    function changed(preedit = false) {
        if (h.field.disabled || h.field.readOnly) return;
        const backward = textarea.selectionDirection === 'backward';
        send({ kind: preedit ? 'preedit' : 'edit', key: h.key, base: h.base, expected: h.value,
            ...replacement(h.value, textarea.value, preedit ? h.composition : '', textarea.selectionEnd),
            anchor: backward ? textarea.selectionEnd : textarea.selectionStart,
            focus: backward ? textarea.selectionStart : textarea.selectionEnd });
    }
    const key = (name, event = {}) => send({ kind: 'key', key: name, code: event.code ?? '', shift: !!event.shiftKey,
        ctrl: !!event.ctrlKey, alt: !!event.altKey, meta: !!event.metaKey, repeat: !!event.repeat });
    listen(textarea, 'input', event => changed(h.composing || event.isComposing));
    listen(textarea, 'compositionstart', () => { h.composing = true; h.composition = ''; });
    listen(textarea, 'compositionupdate', event => {
        h.composition = event.data ?? '';
        queueMicrotask(() => { if (!h.disposed && h.composing) changed(true); });
    });
    listen(textarea, 'compositionend', () => {
        changed();
        h.composing = false; h.composition = '';
        send({ kind: 'sync' });
    });
    listen(password, 'input', () => {
        const text = password.value;
        password.value = '';
        if (text && !password.disabled && !password.readOnly) send({ kind: 'secure', key: h.key, text });
    });
    for (const field of [textarea, password]) {
        listen(field, 'paste', event => {
            event.preventDefault();
            if (field.disabled || field.readOnly) return;
            const data = event.clipboardData;
            h.image = field === textarea ? [...(data?.items ?? [])].find(i => i.kind === 'file' && i.type === 'image/png')?.getAsFile() ?? undefined : undefined;
            try {
                send({ kind: 'paste', text: data?.getData('text/plain') ?? '', html: field === textarea ? data?.getData('text/html') ?? '' : '', image: !!h.image });
            } finally {
                if (h.syncing) queueMicrotask(() => { h.image = undefined; });
                else h.image = undefined;
            }
        });
        listen(field, 'beforeinput', event => {
            if (field.disabled || field.readOnly) { event.preventDefault(); return; }
            if (event.inputType === 'insertLineBreak' || event.inputType === 'insertParagraph') {
                event.preventDefault();
                if (!h.composing && !event.isComposing) key('Enter', { shiftKey: h.shift, code: 'Enter' });
            } else if (field === password && event.inputType.startsWith('delete')) {
                event.preventDefault();
                const forward = event.inputType.endsWith('Forward');
                key(forward ? 'Delete' : 'Backspace',
                    { altKey: event.inputType.includes('Word'), code: forward ? 'Delete' : 'Backspace' });
            }
        });
        listen(field, 'keydown', event => {
            h.shift = event.shiftKey;
            if (event.isComposing || h.composing || event.keyCode === 229) return;
            const chord = event.metaKey || event.ctrlKey;
            if (field.disabled || (field.readOnly && (chord || ['Enter', 'Backspace', 'Delete'].includes(event.key)))) return;
            const editing = ['Enter', 'Tab', 'Escape', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'];
            if (field === password) editing.push('Backspace', 'Delete');
            if (!chord && !editing.includes(event.key)) return;
            if (chord && event.key.toLowerCase() === 'v') return;
            event.preventDefault();
            key(event.key, event);
        });
        listen(field, 'keyup', event => { h.shift = event.shiftKey; });
        listen(field, 'blur', () => { h.shift = false; });
    }
    listen(textarea, 'select', () => {
        if (h.syncing || h.composing || document.activeElement !== textarea) return;
        const backward = textarea.selectionDirection === 'backward';
        send({ kind: 'selection', key: h.key, base: h.base, expected: h.value,
            anchor: backward ? textarea.selectionEnd : textarea.selectionStart,
            focus: backward ? textarea.selectionStart : textarea.selectionEnd });
    });
    // Synchronous copy support for read-only `El::selectable` selections (`Edit › Copy` / ⌘C).
    listen(document, 'copy', event => {
        const active = document.activeElement;
        if (!h.key && h.copyText && event.clipboardData && (active === canvas || active === document.body)) {
            event.preventDefault();
            event.clipboardData.setData('text/plain', h.copyText);
        }
    });
    listen(canvas, 'pointerdown', event => event.preventDefault(), { capture: true });
    listen(canvas, 'keydown', event => {
        if (!browserShortcut(event)) event.preventDefault();
    });
    if (!fitHeight) {
        listen(canvas, 'wheel', event => event.preventDefault(), { passive: false });
        listen(document, 'gesturestart', event => event.preventDefault(), { passive: false });
    }
    queueMicrotask(() => {
        if (!h.disposed && !embedded && document.activeElement === document.body) canvas.focus({ preventScroll: true });
    });
    return h;
}

export function sync(h, key, text, base, anchor, focus, secure) {
    if (h.composing && key === h.key) return;
    h.composing = false;
    h.syncing = true;
    h.key = key; h.base = base; h.value = text;
    h.password.value = '';
    h.field = secure ? h.password : h.textarea;
    if (!key) {
        h.textarea.value = '';
        h.textarea.inputMode = h.password.inputMode = coarse() ? 'none' : 'text';
    } else {
        if (h.textarea.value !== text) h.textarea.value = text;
        if (!secure) {
            const start = Math.min(anchor, focus), end = Math.max(anchor, focus);
            const direction = anchor > focus ? 'backward' : 'forward';
            if (h.textarea.selectionStart !== start || h.textarea.selectionEnd !== end || h.textarea.selectionDirection !== direction) {
                h.textarea.setSelectionRange(start, end, direction);
            }
        }
    }
    h.syncing = false;
}

export function syncCopy(h, text) {
    h.copyText = text;
}

function webLink(url) {
    return url.startsWith('https://') || url.startsWith('http://') || url.startsWith('mailto:');
}

export function syncA11y(h, json) {
    if (h.disposed) return;
    h.restoreFocus ||= ownsFieldFocus(h);
    const items = JSON.parse(json);
    const seen = new Set();
    let cursor = h.a11y.firstElementChild;
    for (const item of items) {
        const tag = item.k === 'input' ? 'div' : item.k === 'heading' ? `h${item.l || 2}` : item.k === 'button' ? 'button' : 'p';
        const key = item.k === 'input' ? `input:${item.p}` : `${tag}:${item.p}`;
        seen.add(key);
        let el = h.a11yNodes.get(key);
        if (!el) {
            el = document.createElement(tag);
            if (item.k === 'input') {
                el.setAttribute('role', 'textbox');
                el.tabIndex = -1;
                el.addEventListener('focus', () => activateA11yInput(h, el));
                el.addEventListener('click', event => {
                    event.preventDefault();
                    if (!el._item?.disabled && document.activeElement !== el) el.focus({ preventScroll: true });
                });
            } else if (tag === 'button') {
                el.type = 'button';
                el.tabIndex = -1;
                el.addEventListener('click', ev => {
                    ev.preventDefault();
                    h.send({ kind: 'a11y_press', path: item.p });
                });
            }
            h.a11yNodes.set(key, el);
        }
        const [x, y, w, bh] = item.r;
        const fs = item.s || 14;
        el.style.cssText = `position:absolute;left:${x}px;top:${y}px;width:${w}px;height:${bh}px;margin:0;padding:0;border:0;background:transparent;color:transparent;font:${fs}px/1.25 sans-serif;overflow:hidden;white-space:pre-wrap;pointer-events:none;`;
        if (item.k === 'input') {
            el._item = item;
            el.setAttribute('aria-label', item.name);
            setOptionalAttribute(el, 'aria-description', item.description);
            setOptionalAttribute(el, 'aria-placeholder', item.placeholder);
            el.setAttribute('aria-multiline', String(item.multiline));
            el.setAttribute('aria-readonly', String(item.readOnly));
            el.setAttribute('aria-disabled', String(item.disabled));
            if (item.invalid) el.setAttribute('aria-invalid', 'true');
            else el.removeAttribute('aria-invalid');

        }
        if (tag === 'button') {
            if (item.x === true || item.x === false) el.setAttribute('aria-expanded', String(item.x));
            else el.removeAttribute('aria-expanded');
        }
        const sig = item.k === 'input' ? item.value ?? '' : item.u?.length ? JSON.stringify(item.u) : item.t ?? '';
        if (el._sig !== sig) {
            el._sig = sig;
            el.replaceChildren();
            if (item.u?.length) {
                for (const run of item.u) {
                    if (run.h) {
                        const a = document.createElement('a');
                        a.textContent = run.t;
                        a.style.cssText = 'color:transparent;text-decoration:none;pointer-events:none;';
                        if (webLink(run.h)) {
                            a.href = run.h;
                            a.target = '_blank';
                            a.rel = 'noopener noreferrer';
                        } else {
                            a.href = '#';
                            a.addEventListener('click', ev => {
                                ev.preventDefault();
                                h.send({ kind: 'a11y_link', path: run.p, link: run.i });
                            });
                        }
                        el.append(a);
                    } else {
                        el.append(document.createTextNode(run.t));
                    }
                }
            } else {
                el.textContent = item.k === 'input' ? item.value ?? '' : item.t ?? '';
            }
        }
        if (cursor !== el) h.a11y.insertBefore(el, cursor);
        else cursor = cursor.nextElementSibling;
    }
    for (const [key, el] of h.a11yNodes) {
        if (!seen.has(key)) {
            if (document.activeElement === el) h.canvas.focus({ preventScroll: true });
            el.remove();
            h.a11yNodes.delete(key);
        }
    }
}

function setOptionalAttribute(el, name, value) {
    if (value) el.setAttribute(name, value);
    else el.removeAttribute(name);
}

function setA11yInputHidden(el, hidden) {
    el.inert = hidden;
    if (hidden) el.setAttribute('aria-hidden', 'true');
    else el.removeAttribute('aria-hidden');
}

function activateA11yInput(h, el) {
    const item = el._item;
    if (!h.disposed && item && !item.disabled) h.send({ kind: 'a11y_focus', path: item.p });
}

function ownsFieldFocus(h) {
    const focused = document.activeElement;
    return focused === h.textarea || focused === h.password || (h.a11y.contains(focused) && focused?._item?.k === 'input');
}

function syncFieldSemantics(h, requestFocus) {
    const semantic = h.a11yNodes.get(`input:${h.key}`);
    const item = semantic?._item;
    const target = h.key && item && !item.disabled ? h.field : null;
    const ownedFocus = h.restoreFocus || ownsFieldFocus(h);
    h.restoreFocus = false;
    if (target) {
        target.setAttribute('aria-label', item.name);
        setOptionalAttribute(target, 'aria-description', item.description);
        setOptionalAttribute(target, 'placeholder', item.placeholder);
        target.setAttribute('aria-multiline', String(item.multiline));
        if (item.invalid) target.setAttribute('aria-invalid', 'true');
        else target.removeAttribute('aria-invalid');
        target.readOnly = item.readOnly;
        target.disabled = false;
        target.removeAttribute('aria-disabled');
        target.inert = false;
        target.removeAttribute('aria-hidden');
        if ((requestFocus || ownedFocus) && document.activeElement !== target) {
            target.inputMode = 'text';
            target.focus({ preventScroll: true });
        }
    } else if ((requestFocus || ownedFocus) && document.activeElement !== h.canvas) {
        h.canvas.focus({ preventScroll: true });
    }
    for (const field of [h.textarea, h.password]) {
        if (field === target) continue;
        field.inert = true;
        field.setAttribute('aria-hidden', 'true');
        field.setAttribute('aria-label', field === h.password ? 'Password input' : 'Text input');
        for (const name of ['aria-description', 'placeholder', 'aria-multiline', 'aria-invalid', 'aria-disabled']) field.removeAttribute(name);
        field.readOnly = false;
        field.disabled = false;
    }
    for (const el of h.a11yNodes.values()) {
        if (el._item?.k === 'input') setA11yInputHidden(el, el === semantic && target !== null);
    }
}

export function activate(h, requestFocus) {
    syncFieldSemantics(h, requestFocus);
}

export async function pastedImage(h, max) {
    const file = h.image;
    h.image = undefined;
    return file && file.size <= max ? new Uint8Array(await file.arrayBuffer()) : undefined;
}

export function destroy(h) {
    h.disposed = true;
    h.image = undefined;
    for (const remove of h.listeners) remove();
    h.password.value = ''; h.textarea.value = ''; h.copyText = '';
    h.a11yNodes.clear();
    h.a11y.remove();
    h.password.remove(); h.textarea.remove();
}
