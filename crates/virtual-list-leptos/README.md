# virtual-list-leptos

Reactive virtual scrolling for Leptos, built on top of `virtual-list`.

This crate has two layers:

- `VirtualizerCore` — a pure state machine for windowing, measurement flushes, scroll anchoring, and scroll-to retry logic
- `use_virtualizer` / `Virtualizer` — the Leptos adapter that binds a DOM container, coalesces scroll with `requestAnimationFrame`, listens for viewport and item resize, and exposes reactive signals for mounted items, rows, total size, padding, dominant item, and scrolling state

## What it gives you

- list virtualizers with per-item estimates and measurement correction
- grid virtualizers with row windowing and width-driven column resolution
- scroll anchoring for measurement changes and zoom-style rescaling
- `scroll_to_offset` and `scroll_to_index` with alignment and retry support, and `on_scroll_idle` for the moment the scroll settles
- reactive `items()`, `rows()`, `total_size()` and `dominant()` signals, plus `scroll_offset()`, `viewport()` and a per-item `item_top()`
- zombie retention: an item (or grid row) the window evicts can be kept rendered for a bounded moment, so the change that moved the window is never visible

## Retiring an evicted item

`VirtualizerOptions::retention` takes one `RetentionPolicy`, and it is the only
line that decides whether the virtualizer kills on eviction or caches:

- `Immediate` (the default) — unmounted in the same tick, DOM and surfaces
  released with it.
- `Grace { ms, max }` — bridged in wall-clock milliseconds, the unit for a
  caller pacing a known operation (the page strips, and a zoom commit that
  raises it for its duration).
- `Frames { frames, max }` — bridged in animation frames, the unit for a caller
  whose problem is frames: a scroll that turns around at once finds its DOM,
  its layout sizes and its painted canvases. Each frame is bounded by
  `FRAME_CEILING_MS`, so a frozen rAF clock still releases the set.

Both bridges are bounded by `max`, oldest-first, and re-entering the window
drops the bridge on the spot. The handle exposes the two ends of the same
mechanism directly: `kill_retained()` ends every bridge whose clock ran out
(what the armed wakers do anyway), and `remove_retained_now()` ends every
bridge THIS tick — for a caller that knows the change the bridge was bought
for has landed.

## Continuous-list sketch

```rust,ignore
let v = use_virtualizer(
    VirtualizerOptions::list(count, move |i| estimate_height(i))
        .gap(24.0)
        .budget(Budget::screenfuls(0.5, 3))
        .initial(Viewport::main_only(800.0), 0.0),
);

view! {
    <div node_ref=list_ref class="overflow-y-auto">
        <div class="relative">
            <div aria-hidden="true"
                style:height=move || format!("{}px", v.total_size().get()) />
            <For
                each=move || v.items().get()
                key=|item| item.index
                children=move |item| view! {
                    <div style=move || format!("position:absolute;top:{}px", item.start)>
                        <Cell index=item.index />
                    </div>
                }
            />
        </div>
    </div>
}

// Once the node exists:
v.bind_container(list_ref.get().unwrap().into());
```

## Grid sketch

Use `VirtualizerOptions::grid(...)` and render `v.rows()` instead of `v.items()`. The geometry kernel resolves how many columns fit, windows by row, and still exposes per-item offsets.

## Relationship to `virtual-list`

`virtual-list` owns the pure geometry and anchor math. `virtual-list-leptos` adds browser-facing concerns: DOM binding, observers, signals, and scroll scheduling.

## License

MIT

<!-- // only the changed file was rewritten -->
