/**
 * Main-card markdown body editor (textarea). Uses Document.replaceBody on commit;
 * canvas regions key the field as `$body`.
 */

/** Strip one trailing newline from bodyMarkdown for display. */
export function bodyMarkdownForEditor(md) {
  if (!md) return "";
  return md.endsWith("\n") ? md.slice(0, -1) : md;
}

/** Map a USV offset to a UTF-16 index for textarea selection. */
export function usvToDomOffset(text, targetUsv) {
  let usv = 0;
  for (let i = 0; i < text.length; i++) {
    if (usv === targetUsv) return i;
    const cp = text.codePointAt(i);
    usv += 1;
    if (cp !== undefined && cp > 0xffff) i++;
  }
  return text.length;
}

/**
 * @param {HTMLElement} host
 * @param {string} initialMarkdown
 * @param {(markdown: string) => void} onChange
 */
export function mountBodyField(host, initialMarkdown, onChange) {
  const textarea = document.createElement("textarea");
  textarea.className = "body-editor";
  textarea.rows = 10;
  textarea.value = bodyMarkdownForEditor(initialMarkdown);
  textarea.spellcheck = false;
  textarea.setAttribute("aria-label", "Memo body");
  host.appendChild(textarea);

  let suppress = false;

  textarea.addEventListener("input", () => {
    if (suppress) return;
    onChange(textarea.value);
  });

  return {
    el: textarea,
    getMarkdown() {
      return textarea.value;
    },
    focus() {
      textarea.focus();
    },
    setCursor(usv) {
      const pos = usvToDomOffset(textarea.value, usv);
      textarea.focus();
      textarea.setSelectionRange(pos, pos);
    },
    setContent(/** @type {string} */ markdown) {
      suppress = true;
      textarea.value = bodyMarkdownForEditor(markdown);
      suppress = false;
    },
  };
}
