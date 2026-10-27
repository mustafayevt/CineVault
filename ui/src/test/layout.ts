/**
 * jsdom has no layout engine: every element measures 0. These helpers give elements a size and
 * add the scroll methods the virtualised views call, so they lay out in tests. Call `stubLayout`
 * in `beforeEach` and `restoreLayout` in `afterEach`.
 */

const layoutSize = { width: 1400, height: 800 };

const originals = new Map<string, PropertyDescriptor | undefined>();

export function stubLayout(): void {
  for (const [name, value] of [
    ["offsetWidth", () => layoutSize.width],
    ["offsetHeight", () => layoutSize.height],
    ["clientWidth", () => layoutSize.width],
    ["clientHeight", () => layoutSize.height],
  ] as const) {
    originals.set(name, Object.getOwnPropertyDescriptor(HTMLElement.prototype, name));
    Object.defineProperty(HTMLElement.prototype, name, { configurable: true, get: value });
  }
  HTMLCanvasElement.prototype.getContext = (() =>
    null) as typeof HTMLCanvasElement.prototype.getContext;
  Element.prototype.scrollTo = function scrollTo(
    this: Element,
    options?: ScrollToOptions | number,
  ) {
    this.scrollTop = typeof options === "object" ? (options.top ?? 0) : 0;
    this.dispatchEvent(new Event("scroll"));
  } as typeof Element.prototype.scrollTo;
}

export function restoreLayout(): void {
  Reflect.deleteProperty(Element.prototype, "scrollTo");
  for (const [name, descriptor] of originals) {
    if (descriptor === undefined) Reflect.deleteProperty(HTMLElement.prototype, name);
    else Object.defineProperty(HTMLElement.prototype, name, descriptor);
  }
  originals.clear();
}
