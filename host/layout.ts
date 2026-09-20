/** Immutable binary layout. Preview and commit use the exact same operation. */
export type Tree = { pane: string } | { axis: "x" | "y"; ratio: number; first: Tree; second: Tree };
export type Edge = "left" | "right" | "top" | "bottom" | "center";
export interface Target { pane: string; edge: Edge }
export interface Rect { x: number; y: number; width: number; height: number }
export const MIME = "application/x-mareader-library-item";
export function leaves(tree: Tree | null): string[] {
  return !tree ? [] : "pane" in tree ? [tree.pane] : [...leaves(tree.first), ...leaves(tree.second)];
}
export function place(tree: Tree | null, target: Target | null, pane: string): Tree {
  if (!tree) return { pane };
  if (!target || !leaves(tree).includes(target.pane)) throw new Error("The target pane has closed");
  if (target.edge !== "center" && leaves(tree).length >= 4) throw new Error("Up to four panes can be open");
  if ("pane" in tree) {
    if (tree.pane !== target.pane) return tree;
    if (target.edge === "center") return { pane };
    const first = target.edge === "left" || target.edge === "top";
    return { axis: target.edge === "left" || target.edge === "right" ? "x" : "y", ratio: .5,
      first: first ? { pane } : tree, second: first ? tree : { pane } };
  }
  // Validate the limit once, then visit only the branch containing the target.
  return leaves(tree.first).includes(target.pane)
    ? { ...tree, first: place(tree.first, target, pane) }
    : { ...tree, second: place(tree.second, target, pane) };
}
export function remove(tree: Tree | null, pane: string): Tree | null {
  if (!tree || "pane" in tree) return tree?.pane === pane ? null : tree;
  const first = remove(tree.first, pane), second = remove(tree.second, pane);
  return !first ? second : !second ? first : { ...tree, first, second };
}
export function rectangles(tree: Tree | null, area: Rect): Map<string, Rect> {
  const result = new Map<string, Rect>();
  function visit(node: Tree, r: Rect): void {
    if ("pane" in node) { result.set(node.pane, r); return; }
    const horizontal = node.axis === "x", span = (horizontal ? r.width : r.height) * node.ratio;
    visit(node.first, { ...r, ...(horizontal ? { width: span } : { height: span }) });
    visit(node.second, { ...r, ...(horizontal ? { x: r.x + span, width: r.width - span } : { y: r.y + span, height: r.height - span }) });
  }
  if (tree) visit(tree, area);
  return result;
}
export function hit(tree: Tree | null, area: Rect, x: number, y: number): Target | null {
  for (const [pane, r] of rectangles(tree, area)) {
    if (x < r.x || y < r.y || x > r.x + r.width || y > r.y + r.height) continue;
    const u = (x-r.x)/r.width, v = (y-r.y)/r.height;
    const closest = Math.min(u, 1-u, v, 1-v);
    const edge: Edge = closest > .25 ? "center" : closest === u ? "left" : closest === 1-u ? "right" : closest === v ? "top" : "bottom";
    return { pane, edge };
  }
  return null;
}
export function internalDrop(value: unknown, session: { token: string; bookId: string } | null): boolean {
  if (!session || !value || typeof value !== "object") return false;
  const p = value as Record<string, unknown>;
  return p.version === 1 && p.source === "library-sidebar" && p.token === session.token && p.bookId === session.bookId;
}
