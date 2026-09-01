// Link-layer construction (annotation -> DOM <a>), split out of renderer.ts.

import type {
  Annotation,
  PageState,
  PDFPageProxy,
  Viewport,
} from "./types";
import { session } from "./state";

async function destToPage(dest: string | unknown[] | null | undefined): Promise<number | null> {
  if (!session.pdf || !dest) return null;
  try {
    const explicit = typeof dest === "string" ? await session.pdf.getDestination(dest) : dest;
    if (!Array.isArray(explicit) || !explicit.length) return null;
    const ref = explicit[0];
    if (typeof ref === "object" && ref !== null) {
      return (await session.pdf.getPageIndex(ref)) + 1;
    }
    if (Number.isInteger(ref)) return (ref as number) + 1;
    return null;
  } catch (_) {
    return null;
  }
}

function safeExternalUrl(raw: string): string | null {
  if (typeof raw !== "string" || !raw) return null;
  let u: URL;
  try {
    u = new URL(raw, globalThis.location ? globalThis.location.href : undefined);
  } catch (_) {
    return null;
  }
  return ["http:", "https:", "mailto:"].includes(u.protocol) ? u.href : null;
}

export async function buildLinkLayer(
  st: PageState,
  viewport: Viewport,
  page: PDFPageProxy | null
): Promise<void> {
  const { host } = st;
  if (!host) return;

  let annots: Annotation[] = [];
  try {
    const src = page || (await session.pdf!.getPage(st.page));
    annots = await src.getAnnotations({ intent: "display" });
    if (!page) {
      try { src.cleanup(); } catch (_) { /* ignore */ }
    }
  } catch (_) {
    annots = [];
  }

  const layer = document.createElement("div");
  layer.className = "linkLayer";

  for (const a of annots) {
    if (!a || a.subtype !== "Link" || !Array.isArray(a.rect)) continue;

    const url = safeExternalUrl(a.url ?? "");
    const linkPage = url ? null : await destToPage(a.dest ?? null);
    if (!url && !linkPage) continue;

    const [x1, y1] = viewport.convertToViewportPoint(a.rect[0]!, a.rect[1]!);
    const [x2, y2] = viewport.convertToViewportPoint(a.rect[2]!, a.rect[3]!);
    const x = Math.min(x1, x2);
    const y = Math.min(y1, y2);
    const w = Math.abs(x2 - x1);
    const h = Math.abs(y2 - y1);
    if (!(w > 0) || !(h > 0)) continue;

    const aEl = document.createElement("a");
    aEl.className = "pdf-link";
    aEl.style.left = x + "px";
    aEl.style.top = y + "px";
    aEl.style.width = w + "px";
    aEl.style.height = h + "px";

    if (url) {
      aEl.href = url;
      aEl.target = "_blank";
      aEl.rel = "noopener noreferrer";
      aEl.title = url;
    } else {
      aEl.href = "#";
      aEl.title = "Go to page " + linkPage;
      const p = linkPage!;
      aEl.dataset.page = String(p);
      aEl.addEventListener("click", (ev) => {
        ev.preventDefault();
        globalThis.dispatchEvent(
          new CustomEvent("pdfreader:navigate", { detail: { page: p } })
        );
      });
    }
    layer.appendChild(aEl);
  }

  const live = host.querySelector(".linkLayer");
  if (live && live.parentNode) {
    live.replaceWith(layer);
  } else {
    host.appendChild(layer);
  }
}
