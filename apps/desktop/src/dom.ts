// <wtl> project: c4 | file: apps/desktop/src/dom.ts | author: watchthelight </wtl>
// The one DOM helper the components share. No virtual DOM: components are factory functions that
// return an element and an update function, and update what changed.

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}
